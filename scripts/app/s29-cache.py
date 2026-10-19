import argparse, contextlib, http.server, importlib.util, json, os, re, shutil, statistics, subprocess, sys, threading, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'scripts', 'app', file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cs, s6 = load('covers_session', 'covers-session.py'), load('s6_shots', 's6-shots.py')
TITLE, LIST = {'S': 'Songs', 'A': 'Album'}, {'S': 'songs', 'A': 'tracks'}
PLAN = ['S-Nc1', 'S-M1', 'S-Nw1', 'A-Nc1', 'A-M1', 'A-Nw1', 'S-Nc2', 'S-M2', 'S-Nw2', 'A-Nc2', 'A-M2', 'A-Nw2', 'S-Nc3', 'A-Nc3', 'S-Nw3', 'A-Nw3']
EXTRA = ['S-M3', 'A-M3']
SPACING, QUIET, CAP, POLL = 70, 6, 90, 0.1
SPAWNED = []
CACHE_KEYS = ('apiHits', 'apiMisses', 'apiRevalidations', 'artHits', 'artMisses')


class Mock(s6.Fixture):
    def do_GET(self):
        base = f'http://127.0.0.1:{self.server.server_address[1]}'
        kind = re.fullmatch(r'/v1/me/library/(?:(songs)|albums/[^/]+/(tracks))\?(?:offset=\d+&)?limit=100', self.path)
        if not kind:
            return super().do_GET()
        rows = [{'id': f'i.{i}', 'type': 'library-songs', 'attributes': {'name': f'Synthetic {i}', 'artistName': 'Fixture Artist', 'artwork': {'url': f'{base}/art/{i % 50}/{{w}}x{{h}}bb.jpg'}}} for i in range(100)]
        body = json.dumps({'data': rows, 'meta': {'total': 100}}).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def cache_bytes(config):
    root = os.path.join(config, 'cache', 'bramley')
    return {sub: sum(os.path.getsize(os.path.join(d, f)) for d, _, names in os.walk(os.path.join(root, sub)) for f in names) for sub in ('art', 'api')}


def watch(p, t0, snap, which):
    first, last, status = {}, {}, {}
    while True:
        t, s = time.monotonic() - t0, snap()
        status = s or status
        lib = s.get('library', {})
        for key, value in (('frame', s.get('framesPainted', 0)), ('route', s.get('route') == TITLE[which]), ('list', lib.get(LIST[which], {}).get('loaded', 0)),
                           ('first', lib.get('artTextures', 0))):
            if value:
                first.setdefault(key, round(t, 2))
        if last.get('lib', (None, 0))[0] != lib.get('artTextures', 0):
            last['lib'] = (lib.get('artTextures', 0), round(t, 2))
        if 'first' in first and t - last['lib'][1] >= QUIET:
            return {**first, 'all': last['lib'][1], 'artTextures': last['lib'][0]}, status, None
        if t > CAP or p.poll() is not None:
            return first, status, 'exited' if p.poll() is not None else 'cap'
        time.sleep(POLL)


def launch(a, tag, which, binary, env, config, extra, out):
    route = 'songs' if which == 'S' else f'album:{open(a.album_id_file).read().strip()}'
    with cs.session(out, 's29') as (sock, sink, _):
        full = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **env)
        full.pop('DISPLAY', None)
        cs.assert_isolated(full)
        st = os.path.join(out, tag + '.status.json')
        if not a.mock and SPAWNED and time.monotonic() - SPAWNED[-1] < SPACING:
            raise RuntimeError('refusing: the last launch was under 70 s ago')
        SPAWNED.append(time.monotonic())
        with open(a.ledger, 'a') as f:
            f.write(json.dumps({'launch': tag, 'spawn': time.strftime('%Y-%m-%dT%H:%M:%S%z'), 'mono': round(time.monotonic(), 1)}) + '\n')
        t0 = time.monotonic()
        p = subprocess.Popen([os.path.abspath(binary), '--status', st, '--route', route, '--size', '1100x720'] + extra, env=full, stdout=open(os.path.join(out, tag + '.log'), 'w'), stderr=subprocess.STDOUT, start_new_session=True)
        guard = cs.Guard(p.pid, sink)
        guard.start()
        try:
            result, status, why = watch(p, t0, lambda: cs.status(st) or {}, which)
        finally:
            guard.stop = True
            p.terminate()
            try:
                p.wait(15)
            except subprocess.TimeoutExpired:
                p.kill()
        for leftover in (st, os.path.join(out, tag + '.log')):
            if os.path.exists(leftover):
                os.remove(leftover)
    cache = status.get('cache')
    ok = why is None and not guard.killed and all(result.get(k) for k in ('frame', 'route', 'list', 'first')) and result.get('artTextures', 0) > 0
    return {'launch': tag, 'ok': ok, 'why': why or (guard.killed and guard.killed['why']), **result, 'cache': cache and {k: cache.get(k) for k in CACHE_KEYS}, 'bytes': cache_bytes(config)}


def judge(row):
    kind, cache = row['launch'].split('-')[2][:2], row['cache']
    if kind == 'Nc':
        return bool(cache) and cache['apiHits'] == 0 and cache['apiMisses'] >= 1
    if kind == 'Nw':
        return bool(cache) and cache['apiHits'] >= 1 and cache['artHits'] >= 1
    return cache is None or cache['apiHits'] == 0


def medians(rows):
    out = {}
    for which in 'SA':
        for arm in ('Nc', 'Nw', 'M'):
            mine = [r for r in rows if r['ok'] and f'-{which}-{arm}' in r['launch']]
            out[f'{which}-{arm}'] = {m: round(statistics.median([r[m] for r in mine]), 2) for m in ('list', 'all')} | {'n': len(mine)} if mine else None
    return out


def verdict(med):
    checks, ratios = [], {}
    for which in 'SA':
        cold, warm, main = med[f'{which}-Nc'], med[f'{which}-Nw'], med[f'{which}-M']
        if not (cold and warm and main) or min(cold['n'], warm['n']) < 3 or main['n'] < 2:
            return {'ship': None, 'why': f'not enough valid launches on {which}'}
        ratios[which] = {'warm/cold': round(warm['all'] / cold['all'], 3), 'cold/main': round(cold['all'] / main['all'], 3), 'mainN': main['n']}
        checks += [warm['all'] <= 0.5 * cold['all'], cold['all'] <= 1.2 * main['all']]
    return {'ship': all(checks), 'ratios': ratios}


def run(a):
    server, extra = None, []
    if a.mock:
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Mock)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f'http://127.0.0.1:{server.server_address[1]}'
        extra = ['--api-base', base, '--developer-token-url', base + '/developer-token.json']
    os.makedirs(a.out, exist_ok=True)
    plan = EXTRA if a.extra else (a.only or PLAN)
    rows, homes = [], {}
    with contextlib.ExitStack() as stack:
        for n, name in enumerate(plan, 1):
            which, arm = name.split('-')[0], name.split('-')[1]
            tag = f'{n:02d}-{name}'
            if SPAWNED and not a.mock:
                time.sleep(max(0, SPAWNED[-1] + SPACING - time.monotonic()))
            pair = f'{which}{arm[2:]}'
            if arm.startswith('Nw'):
                iso, config = homes.pop(pair)
                token = os.path.join(config, 'cache', 'bramley', 'developer-token.json')
                if os.path.exists(token):
                    os.remove(token)
            else:
                iso, config = stack.enter_context(cs.isolated(f's29-{pair}-{arm}', not a.mock))
                if a.mock:
                    for f, v in (('developer-token', s6.FIXTURE_DEVELOPER), ('music-user-token', s6.FIXTURE_USER)):
                        open(os.path.join(config, f), 'w').write(v)
                if arm.startswith('Nc'):
                    homes[pair] = (iso, config)
            binary = a.binary_main if arm.startswith('M') else a.binary_new
            for _ in range(180):
                try:
                    row = launch(a, tag, which, binary, iso, config, extra, a.out)
                    break
                except RuntimeError as e:
                    if 'refusing' not in str(e):
                        raise
                    time.sleep(20)
            else:
                raise RuntimeError('the GPU session lock stayed taken')
            row['did_it_run'] = row['ok'] and judge(row)
            row['ok'] = row['did_it_run']
            rows.append(row)
            print(json.dumps(row), flush=True)
    summary = {'medians': medians(rows), 'verdict': verdict(medians(rows)) if len(rows) >= 16 else None, 'launches': len(rows)}
    json.dump({'rows': rows, **summary}, open(os.path.join(a.out, 'rows.json'), 'w'), indent=1)
    print(json.dumps(summary), flush=True)
    if server:
        server.shutdown()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--binary-new', required=True)
    ap.add_argument('--binary-main', required=True)
    ap.add_argument('--album-id-file', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--ledger', required=True)
    ap.add_argument('--mock', action='store_true')
    ap.add_argument('--extra', action='store_true')
    ap.add_argument('--only', nargs='*')
    run(ap.parse_args())


if __name__ == '__main__':
    main()
