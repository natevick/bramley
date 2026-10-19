import argparse, contextlib, hashlib, importlib.util, json, math, os, random, re, socket, statistics, subprocess, sys, time, types

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
SPACING, MAX_WARMS, MAX_LAUNCHES, ROW_PT = 70, 6, 10, 40
PAGE = '/v1/me/library/albums'
LINE = re.compile(r'bramley warm: requests=(\d+) cached=(\d+) skipped=(\d+) stopped=([a-z-]+)')


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'scripts', 'app', file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


s29 = load('s29_cache', 's29-cache.py')
cs = s29.cs
STATE = {'last': None, 'warms': 0, 'launches': 0}


def digest(*parts):
    sha = hashlib.sha256()
    for part in parts:
        sha.update(part.encode() + b'\0')
    return sha.digest()[:16].hex()


class Cache:
    def __init__(self, config, user):
        self.root, self.scope = os.path.join(config, 'cache', 'bramley'), digest(user)

    def api(self, request):
        path = os.path.join(self.root, 'api', digest('api', self.scope, request))
        return json.load(open(path)) if os.path.exists(path) else None

    def tracks_file(self, album):
        return os.path.join(self.root, 'api', digest('api', self.scope, f'/v1/me/library/albums/{album}/tracks?limit=100'))

    def has_art(self, url):
        return os.path.exists(os.path.join(self.root, 'art', digest('art', url)))

    def scale(self):
        text = open(os.path.join(self.root, 'scale')).read().strip()
        return float(text)

    def pages(self):
        request, found = f'{PAGE}?limit=100', []
        while request and (body := self.api(request)):
            found += body['data']
            nxt = body.get('next')
            request = nxt and (nxt + ('?limit=100' if 'limit=' not in nxt else '?'))
        return found

    def warmed(self, album, row_px):
        body = self.api(f'/v1/me/library/albums/{album}/tracks?limit=100')
        if not body or not body['data']:
            return False
        templates = {t['attributes']['artwork']['url'] for t in body['data'][:24] if t['attributes'].get('artwork', {}).get('url')}
        urls = [t.replace('{w}', str(row_px)).replace('{h}', str(row_px)) for t in templates]
        return bool(urls) and all(self.has_art(u) for u in urls)


def px(points, scale):
    return math.floor(points * scale + 0.5)


def pause():
    if STATE['last'] is not None:
        time.sleep(max(0, STATE['last'] + SPACING - time.monotonic()))
    STATE['last'] = time.monotonic()


def app_running():
    path = os.path.join(os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}'), 'bramley', 'instance.sock')
    with socket.socket(socket.AF_UNIX) as probe:
        try:
            probe.connect(path)
            return True
        except OSError:
            return False


def warm_env(env):
    full = dict(os.environ, XDG_RUNTIME_DIR=f'/run/user/{os.getuid()}', **env)
    for key in ('DISPLAY', 'WAYLAND_DISPLAY'):
        full.pop(key, None)
    cs.assert_isolated(full)
    return full


def note(a, entry):
    with open(a.ledger, 'a') as f:
        f.write(json.dumps(entry) + '\n')


def parse(out):
    found = LINE.search(out)
    return found and dict(zip(('requests', 'cached', 'skipped'), map(int, found.groups()[:3])), stopped=found[4])


def warm_run(a, env, tag):
    if STATE['warms'] >= MAX_WARMS or app_running():
        raise RuntimeError('refusing: warm budget spent or an app is running')
    pause()
    STATE['warms'] += 1
    note(a, {'warm': tag, 'start': time.strftime('%Y-%m-%dT%H:%M:%S%z')})
    done = subprocess.run([os.path.abspath(a.binary), 'warm'], env=warm_env(env), capture_output=True, text=True, timeout=240)
    return {'warm': tag, 'exit': done.returncode, **(parse(done.stdout) or {'stopped': 'unparsed'})}


def launch(a, tag, which, album, env, config):
    if STATE['launches'] >= MAX_LAUNCHES:
        raise RuntimeError('refusing: launch budget spent')
    STATE['launches'] += 1
    pause()
    s29.SPAWNED[:] = []
    open(a.album_file, 'w').write(album)
    ns = types.SimpleNamespace(album_id_file=a.album_file, mock=False, ledger=a.ledger)
    for _ in range(180):
        try:
            row = s29.launch(ns, tag, which, a.binary, env, config, [], a.out)
            break
        except RuntimeError as e:
            if 'refusing' not in str(e):
                raise
            time.sleep(20)
    else:
        raise RuntimeError('the GPU session lock stayed taken')
    STATE['last'] = time.monotonic()
    return row


def token(a):
    return open(os.path.expanduser('~/.config/bramley/music-user-token')).read().strip()


def run(a):
    os.makedirs(a.out, exist_ok=True)
    rows, warms = [], []
    with contextlib.ExitStack() as stack:
        env_w, config_w = stack.enter_context(cs.isolated('s32-warm', True))
        build = {'tree': subprocess.run(['git', 'rev-parse', 'HEAD^{tree}'], cwd=ROOT, capture_output=True, text=True).stdout.strip(),
                 'dirty': bool(subprocess.run(['git', 'status', '--porcelain'], cwd=ROOT, capture_output=True, text=True).stdout.strip()),
                 'sha256': hashlib.sha256(open(a.binary, 'rb').read()).hexdigest()}
        note(a, {'build': build})
        seed = launch(a, 'S', 'S', '', env_w, config_w)
        print(json.dumps(seed), flush=True)
        if not seed['ok'] or not os.path.exists(os.path.join(config_w, 'cache', 'bramley', 'scale')):
            raise RuntimeError('the seed launch did not record a display scale; no warm run spent')
        for n in range(1, a.warms + 1):
            result = warm_run(a, env_w, f'W{n}')
            warms.append(result)
            print(json.dumps(result), flush=True)
            if result.get('exit') != 0 or result['stopped'] not in ('complete', 'budget'):
                raise RuntimeError(f'warm stopped for a reason that yields no data: {result}')
            if result['stopped'] == 'complete':
                break
        cache = Cache(config_w, token(a))
        row_px = px(ROW_PT, cache.scale())
        library = [i['id'] for i in cache.pages() if i['id'].startswith('l.')]
        population = sorted(i for i in library if cache.warmed(i, row_px))
        rng = random.Random(32)
        if len(population) < 6:
            raise RuntimeError(f'only {len(population)} warmed albums of {len(library)}')
        picks = rng.sample(population, 6)
        cold, warm = picks[:3], picks[3:]
        coverage = {'warmed': len(population), 'library': len(library)}
        print(json.dumps({'coverage': coverage}), flush=True)
        for n in range(3):
            iso_c, config_c = stack.enter_context(cs.isolated(f's32-cold{n}', True))
            before = Cache(config_c, token(a)).warmed(cold[n], row_px)
            row = launch(a, f'C{n + 1}', 'A', cold[n], iso_c, config_c)
            row.update(arm='C', existed_before=before)
            rows.append(row)
            print(json.dumps(row), flush=True)
            dev = os.path.join(config_w, 'cache', 'bramley', 'developer-token.json')
            if os.path.exists(dev):
                os.remove(dev)
            before = cache.warmed(warm[n], row_px)
            row = launch(a, f'W{n + 1}', 'A', warm[n], env_w, config_w)
            row.update(arm='W', existed_before=before)
            rows.append(row)
            print(json.dumps(row), flush=True)
        spare = [i for i in population if i not in picks][:5]
        t = tail(a, env_w, config_w, [cache.tracks_file(i) for i in spare])
    summary = judge(rows, t, warms, coverage, build)
    json.dump({'seed': seed, 'rows': rows, 'warms': warms, 'tail': t, 'summary': summary}, open(os.path.join(a.out, 'rows.json'), 'w'), indent=1)
    print(json.dumps(summary), flush=True)


def tail(a, env, config, stale):
    if app_running():
        raise RuntimeError('refusing: an app is running')
    for path in stale:
        old = time.time() - 7 * 3600
        os.utime(path, (old, old))
    pause()
    STATE['warms'] += 1
    note(a, {'warm': 'T', 'start': time.strftime('%Y-%m-%dT%H:%M:%S%z')})
    started = time.monotonic()
    proc = subprocess.Popen([os.path.abspath(a.binary), 'warm'], env=warm_env(env), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    time.sleep(2)
    dev = os.path.join(config, 'cache', 'bramley', 'developer-token.json')
    if os.path.exists(dev):
        os.remove(dev)
    STATE['last'] = None
    still_running = proc.poll() is None
    row = launch(a, 'T', 'S', '', env, config)
    out = proc.communicate(timeout=60)[0]
    return {'row': row, 'warm': parse(out), 'warmWasRunningAtSpawn': still_running, 'warmSeconds': round(time.monotonic() - started, 1)}


def judge(rows, t, warms, coverage, build):
    valid = lambda arm: [r for r in rows if r['arm'] == arm and r['ok'] and (r['existed_before'] == (arm == 'W'))]
    cold, warm = valid('C'), valid('W')
    frames = [r['frame'] for r in cold + warm]
    med = lambda rs, key: round(statistics.median([r[key] for r in rs]), 2) if rs else None
    spread = lambda rs: round(max(r['all'] for r in rs) - min(r['all'] for r in rs), 2) if rs else None
    frame_bar = round(1.2 * statistics.median(frames), 2) if frames else None
    row = t['row']
    return {
        'coverage': coverage,
        'build': build,
        'warmRuns': [{k: w.get(k) for k in ('requests', 'cached', 'skipped', 'stopped')} for w in warms + [t['warm'] or {}]],
        'cold': {'n': len(cold), 'median': med(cold, 'all'), 'spread': spread(cold)},
        'warm': {'n': len(warm), 'median': med(warm, 'all'), 'spread': spread(warm)},
        'bar1': bool(warm) and len(warm) >= 2 and med(warm, 'all') <= 0.5,
        'bar2': all(w.get('requests', 99) <= 60 for w in warms + [t['warm'] or {}]),
        'bar3': {'frame': row.get('frame'), 'limit': frame_bar, 'pass': bool(row['ok'] and frame_bar and row['frame'] <= frame_bar and t['warm'] and t['warm']['stopped'] == 'app-running'), 'warmWasRunning': t['warmWasRunningAtSpawn']},
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--binary', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--ledger', required=True)
    ap.add_argument('--album-file', required=True)
    ap.add_argument('--warms', type=int, default=5)
    run(ap.parse_args())


if __name__ == '__main__':
    main()
