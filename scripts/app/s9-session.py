import argparse, http.server, importlib.util, json, os, re, shutil, statistics, subprocess, sys, threading, time, urllib.error, urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'scripts', 'app', file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cs, s6 = load('covers_session', 'covers-session.py'), load('s6_shots', 's6-shots.py')
API, UA = 'https://api.music.apple.com', 'Bramley/0.1.0 (linux)'
SCRATCH = os.path.join(ROOT, 'target', 'tmp', 's9')
NAMES = os.path.join(SCRATCH, 'names.txt')
PRIVATE = os.path.join(ROOT, 'target', 'screens', 'PRIVATE-real-art', 's9')
PUBLIC = os.path.join(ROOT, 'docs', 'screens', 's9')
KEYS = ('state', 'position', 'banner', 'engineStarted', 'framesPainted', 'glRenderer', 'route', 'library')
ROWS_BAR, ART_CAP, DELTA_BAR = 30, 96, 10.0


def api(path, tokens):
    for _ in range(4):
        req = urllib.request.Request(API + path, headers={'Authorization': 'Bearer ' + tokens[0], 'Music-User-Token': tokens[1], 'User-Agent': UA})
        try:
            return json.load(urllib.request.urlopen(req, timeout=20))
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return {'data': [], 'meta': {'total': 0}}
            if e.code != 429:
                raise RuntimeError(f'HTTP {e.code} on a library call') from None
            time.sleep(2)
    raise RuntimeError('rate limited')


def every(path, tokens):
    items, total = [], None
    while path:
        body = api(path + ('&' if '?' in path else '?') + 'limit=100', tokens)
        items, total, path = items + body['data'], body.get('meta', {}).get('total', total), body.get('next')
    return items, total


def discover(config):
    tokens = [open(os.path.join(config, f)).read().strip() for f in ('developer-token', 'music-user-token')]
    playlists, _ = every('/v1/me/library/playlists', tokens)
    sizes = {p['id']: api(f"/v1/me/library/playlists/{p['id']}/tracks?limit=1", tokens).get('meta', {}).get('total', 0) for p in playlists}
    largest = max(sizes, key=sizes.get)
    tracks, total = every(f'/v1/me/library/playlists/{largest}/tracks', tokens)
    songs, songs_total = every('/v1/me/library/songs', tokens)
    names = {a.get(k) for r in playlists + tracks + songs for a in [r.get('attributes', {})] for k in ('name', 'artistName', 'albumName')}
    names = sorted(n for n in names if n and len(n) >= 4)
    os.makedirs(SCRATCH, exist_ok=True)
    with os.fdopen(os.open(NAMES, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), 'w') as f:
        f.write('\n'.join(names) + '\n')
    last = tracks[-1]
    ids = {last['id'], last.get('attributes', {}).get('playParams', {}).get('catalogId')}
    return {'playlist': largest, 'playlists': len(playlists), 'tracksTotal': total, 'tracksFetched': len(tracks), 'songsTotal': songs_total, 'songsFetched': len(songs), 'names': len(names)}, ids - {None}, names


def launch(binary, env, args, out, name):
    st = os.path.join(out, name + '.status.json')
    if os.path.exists(st):
        os.remove(st)
    p = subprocess.Popen([binary, '--status', st] + args, env=env, stdout=open(os.path.join(out, name + '.log'), 'w'), stderr=subprocess.STDOUT, start_new_session=True)
    return p, lambda: cs.status(st) or {}


def pss(p, n):
    rows = []
    for _ in range(n):
        rows.append(sum(cs.rollup(q)['Pss'] for q in [p.pid] + cs.children(p.pid)) / 1024)
        time.sleep(1)
    return rows


def stop(p, guard):
    if p.poll() is None:
        p.terminate()
        try:
            p.wait(15)
        except subprocess.TimeoutExpired:
            p.kill()
    guard.stop = True


def run(binary, env, sink, out, name, args, steps):
    p, snap = launch(binary, env, args, out, name)
    guard = cs.Guard(p.pid, sink)
    guard.start()
    res = {'name': name}
    try:
        cs.wait_for(lambda: snap().get('framesPainted', 0) > 0, 30, 'first frame')
        steps(p, snap, res)
    except RuntimeError as e:
        res['error'] = str(e)
    finally:
        stop(p, guard)
        final = snap()
        status_file = os.path.join(out, name + '.status.json')
        if os.path.exists(status_file):
            os.remove(status_file)
        res.update(final={k: final.get(k) for k in KEYS}, guardKilled=guard.killed and guard.killed['why'], sinksSeen=sorted(map(str, guard.seen)))
        json.dump(res, open(os.path.join(out, name + '.json'), 'w'), indent=1)
    return res


def end_reached(lib, key, total):
    shelf = lib.get(key, {})
    return shelf.get('loaded') == shelf.get('total') == total and lib.get('shownTo') == shelf.get('loaded') and shelf.get('pages', 0) > 1


def bars(lib):
    return lib.get('rowsBuiltMax', 99) <= ROWS_BAR and 0 < lib.get('artTextures', 0) <= ART_CAP


def leak_grep(names, out):
    texts = [subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True).stdout for cmd in (['git', 'diff', 'main..HEAD'], ['git', 'log', 'main..HEAD', '--format=%B'])]
    texts += [open(os.path.join(out, f), errors='replace').read() for f in os.listdir(out) if f.endswith('.log')]
    count = lambda text: sum(text.count(n) for n in names)
    hits = {n: sum(t.count(n) for t in texts) for n in names}
    hits = {n: c for n, c in hits.items() if c}
    print('leak grep hits (stderr only):', dict(sorted(hits.items(), key=lambda kv: -kv[1])), file=sys.stderr)
    return {'leakHits': sum(hits.values()), 'leakGrepCanFail': count(texts[0] + names[len(names) // 2]) > count(texts[0]), 'logsSearched': len(texts) - 2}


def live(a, out, sock, sink):
    try:
        with cs.isolated('s9-live', True) as (iso, config):
            found, played_ids, names = discover(config)
            found, results = live_runs(a, out, sock, sink, iso, found, played_ids)
            found.update(leak_grep(names, out))
            return found, results
    finally:
        if os.path.exists(NAMES):
            os.remove(NAMES)


def live_runs(a, out, sock, sink, iso, found, played_ids):
    env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **iso)
    env.pop('DISPLAY', None)
    cs.assert_isolated(env)
    lib = lambda snap: snap().get('library', {})

    def home(p, snap, res):
        time.sleep(8)
        res['pss'] = round(statistics.median(pss(p, 20)), 1)

    def songs(p, snap, res, scroll=True):
        cs.wait_for(lambda: lib(snap).get('songs', {}).get('pages', 0) > 0, 30, 'the first songs page')
        before = pss(p, 20)
        res['r2LibraryPss'], res['before'] = round(statistics.median(before), 1), round(statistics.median(before[-10:]), 1)
        try:
            cs.wait_for(lambda: end_reached(lib(snap), 'songs', found['songsTotal']), 240 if scroll else 30, 'the end of Songs')
        except RuntimeError:
            pass
        time.sleep(3)
        res['after'] = round(statistics.median(pss(p, 10)), 1)
        res['delta'] = round(res['after'] - res['before'], 1)
        res['L3'] = end_reached(lib(snap), 'songs', found['songsTotal']) and bars(lib(snap)) and res['delta'] <= DELTA_BAR

    def playlist(p, snap, res):
        if not cs.sink_exists(sink) or cs.default_sink() in (None, sink):
            raise RuntimeError('audio not safe before play')
        cs.wait_for(lambda: end_reached(lib(snap), 'tracks', found['tracksTotal']), 120, 'the end of the playlist')
        res['L1'] = found['tracksTotal'] >= 100 and bars(lib(snap))
        cs.wait_for(lambda: snap().get('state') == 'Playing' and (snap().get('position') or 0) > 0, 30, 'playing')
        cs.wait_for(lambda: snap().get('itemId'), 10, 'a now-playing item')
        res['clickedIsPlaying'] = snap().get('itemId') in played_ids
        title = json.loads(subprocess.run([a.binary, 'now'], env=env, capture_output=True, text=True).stdout or '{}').get('title')
        res['nowHasTitle'] = bool(title)
        res['titleInStatusFile'] = bool(title) and title in open(os.path.join(out, 'playlist.status.json')).read()
        res['r1LibraryPss'] = round(statistics.median(pss(p, 20)), 1)
        res['L2'] = res['clickedIsPlaying'] and snap().get('state') == 'Playing' and res['nowHasTitle'] and not res['titleInStatusFile']

    common = ['--size', '1100x720']
    results = [run(a.binary, env, sink, out, 'home', common, home),
               run(a.binary, env, sink, out, 'playlist', common + ['--route', f"playlist:{found['playlist']}", '--scroll-end', '6', '--click-last'], playlist),
               run(a.binary, env, sink, out, 'songs', common + ['--route', 'songs', '--scroll-end', '35'], songs),
               run(a.binary, env, sink, out, 'sabotage-s1', common + ['--route', 'songs'], lambda p, s, r: songs(p, s, r, False))]
    os.makedirs(PRIVATE, exist_ok=True)
    for route in ('songs', 'albums', f"playlist:{found['playlist']}"):
        png = os.path.join(PRIVATE, route.split(':')[0] + '.png')
        subprocess.run([a.binary, '--route', route, '--screenshot', png, '--after', '12'] + common, env=env, capture_output=True, timeout=60)
    found['privateShots'] = sorted(os.listdir(PRIVATE))
    found.pop('playlist')
    return found, results


class Fixture(s6.Fixture):
    def do_GET(self):
        base = f'http://127.0.0.1:{self.server.server_address[1]}'
        shelf = re.fullmatch(r'/v1/me/library/(songs|playlists|albums)\?(?:offset=(\d+)&)?limit=100', self.path)
        if not shelf:
            return super().do_GET()
        kind, at = shelf[1], int(shelf[2] or 0)
        total = {'songs': 250, 'albums': 40, 'playlists': 12}[kind]
        rows = [{'id': f'i.{i}', 'type': f'library-{kind}', 'attributes': {'name': f'Synthetic {kind[:-1].title()} {i + 1}', 'artistName': 'Fixture Artist', 'artwork': {'url': f'{base}/art/{i % 50}/{{w}}x{{h}}bb.jpg'}}} for i in range(at, min(at + 100, total))]
        page = {'data': rows, 'meta': {'total': total}, **({'next': f'/v1/me/library/{kind}?offset={at + 100}'} if at + 100 < total else {})}
        body = json.dumps(page).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def synthetic(a, out, sock, sink):
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    try:
        with cs.isolated('s9-synthetic', False) as (iso, config):
            for f, v in (('developer-token', s6.FIXTURE_DEVELOPER), ('music-user-token', s6.FIXTURE_USER)):
                open(os.path.join(config, f), 'w').write(v)
            env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **iso)
            env.pop('DISPLAY', None)
            report = {}
            for route in ('songs', 'albums'):
                png, st = os.path.join(out, f'{route}-synthetic.png'), os.path.join(out, f'{route}-synthetic.status.json')
                cmd = [a.binary, '--status', st, '--route', route, '--screenshot', png, '--after', '6', '--size', '1100x720', '--api-base', base, '--developer-token-url', base + '/developer-token.json']
                subprocess.run(cmd, env=env, capture_output=True, timeout=60)
                lib = (cs.status(st) or {}).get('library', {})
                report[route] = os.path.exists(png) and lib.get('artTextures', 0) > 0 and s6.png_facts(png)['distinctRows'] > 1
            if report['songs']:
                os.makedirs(PUBLIC, exist_ok=True)
                shutil.copy(os.path.join(out, 'songs-synthetic.png'), PUBLIC)
            return report
    finally:
        server.shutdown()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('mode', choices=('live', 'synthetic'))
    ap.add_argument('binary')
    ap.add_argument('--out', required=True)
    a = ap.parse_args()
    a.binary = os.path.abspath(a.binary)
    os.makedirs(a.out, exist_ok=True)
    with cs.session(a.out, f'bramley-s9-{a.mode}') as (sock, sink, _):
        report = live(a, a.out, sock, sink) if a.mode == 'live' else synthetic(a, a.out, sock, sink)
    json.dump(report, open(os.path.join(a.out, a.mode + '.json'), 'w'), indent=1)
    print(json.dumps(report, indent=1))


if __name__ == '__main__':
    main()
