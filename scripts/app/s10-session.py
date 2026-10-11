import argparse, http.server, importlib.util, json, os, shutil, statistics, subprocess, threading, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'scripts', 'app', file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


s9 = load('s9_session', 's9-session.py')
cs, s6 = s9.cs, s9.s6
A = '6781024437'
SCRATCH = os.path.join(ROOT, 'target', 'tmp', 's10')
NAMES = os.path.join(SCRATCH, 'names.txt')
PRIVATE = os.path.join(ROOT, 'target', 'screens', 'PRIVATE-real-art', 's10')
PUBLIC = os.path.join(ROOT, 'docs', 'screens', 's10')
QUEUE_KEYS = ('"queue"', '"queuePosition"', '"queueLength"')
COMMON = ['--size', '1100x720']


def discover(config):
    tokens = [open(os.path.join(config, f)).read().strip() for f in ('developer-token', 'music-user-token')]
    sf = s9.api('/v1/me/storefront', tokens)['data'][0]['id']
    song = s9.api(f'/v1/catalog/{sf}/songs/{A}?include=albums', tokens)['data'][0]
    album = song['relationships']['albums']['data'][0]['id']
    tracks = s9.api(f'/v1/catalog/{sf}/albums/{album}/tracks', tokens)['data']
    others = [t['id'] for t in tracks if t['id'] != A and t.get('type') == 'songs']
    songs = [r for at in (0, 100) for r in s9.api(f'/v1/me/library/songs?limit=100&offset={at}', tokens)['data']]
    names = {r.get('attributes', {}).get(k) for r in songs + tracks + [song] for k in ('name', 'artistName', 'albumName')}
    names = sorted(n for n in names if n and len(n) >= 4)
    os.makedirs(SCRATCH, exist_ok=True)
    with os.fdopen(os.open(NAMES, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), 'w') as f:
        f.write('\n'.join(names) + '\n')
    ids = lambda r: {r['id'], r.get('attributes', {}).get('playParams', {}).get('catalogId')} - {None}
    return {'queue': [A] + others[:3], 'x': others[3], 'song0': ids(songs[0]), 'song100': ids(songs[100]) if len(songs) > 100 else set(), 'songs': len(songs)}, names


def live(a, out, sock, sink):
    try:
        with cs.isolated('s10-live', True) as (iso, config):
            found, names = discover(config)
            env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **iso)
            env.pop('DISPLAY', None)
            cs.assert_isolated(env)
            report = live_runs(a, out, sink, env, found, names)
            report.update(s9.leak_grep(names, out))
            return report
    finally:
        if os.path.exists(NAMES):
            os.remove(NAMES)


def live_runs(a, out, sink, env, found, names):
    q, x = found['queue'], found['x']

    def cli(*args):
        r = subprocess.run([a.binary, *args], env=env, capture_output=True, text=True, timeout=20)
        return r.returncode, json.loads((r.stdout.splitlines() or ['{}'])[0] or '{}')

    now = lambda: cli('now')[1]

    def until(pred, timeout, what):
        return cs.wait_for(lambda: (lambda n: n if pred(n) else None)(now()), timeout, what)

    playing = lambda n, ids: n.get('itemId') in ids and n.get('state') == 'Playing' and (n.get('position') or 0) > 0

    def clean(name, res):
        text = open(os.path.join(out, name + '.status.json')).read()
        res['statusClean'] = not any(k in text for k in QUEUE_KEYS) and not any(n in text for n in names)

    def safe():
        if not cs.sink_exists(sink) or cs.default_sink() in (None, sink):
            raise RuntimeError('audio not safe before play')

    def four(res):
        safe()
        res['exits'] = [cli('play', q[0])[0]]
        until(lambda n: playing(n, {q[0]}), 40, 'the first song playing')
        res['exits'] += [cli('queue', 'add', i)[0] for i in q[1:]]
        n = until(lambda n: [e['id'] for e in n.get('queue', [])] == q, 20, 'the queue of four')
        res['queueOk'] = (n.get('queuePosition'), n.get('queueLength')) == (0, 4)
        res['nowQueueTitles'] = any('title' in e for e in n['queue'])

    def f8(p, snap, res):
        four(res)
        seen = [q[0]]
        for expect in q[1:]:
            cli('seek', str(max(0.0, (now().get('duration') or 30) - 5)))
            end = time.time() + 30
            while time.time() < end:
                n = now()
                if n.get('itemId') and n['itemId'] != seen[-1]:
                    seen.append(n['itemId'])
                if playing(n, {expect}):
                    break
                time.sleep(0.5)
        res['seen'] = seen
        res['F8'] = seen == q and res['exits'] == [0] * 4 and res['queueOk'] and not res['nowQueueTitles']
        res['F8S'] = seen == [q[0], q[2], q[1], q[3]]
        clean('f8', res)

    def jump(p, snap, res):
        four(res)
        n = until(lambda n: playing(n, {q[3]}), 30, 'item 3 playing')
        res['J'] = n.get('queuePosition') == 3
        clean('jump', res)

    def topup(p, snap, res, sabotage=False):
        safe()
        cs.wait_for(lambda: snap().get('engineStarted'), 60, 'the click')
        if sabotage:
            cs.wait_for(lambda: cli('queue', 'add', x)[0] == 0, 20, 'queue add X')
        res['clickedPlays'] = bool(until(lambda n: playing(n, found['song0']) or (n.get('queuePosition') or 0) > 0, 60, 'the clicked song'))
        res['pssWindow'] = round(statistics.median(s9.pss(p, 10)), 1)
        n = until(lambda n: n.get('queuePosition') == 98, 90, 'position 98')
        res['lengthAt98'] = n.get('queueLength')
        try:
            n = until(lambda n: (n.get('queueLength') or 0) >= 200, 60, 'the top-up')
            res['pssTopped'] = round(statistics.median(s9.pss(p, 10)), 1)
        except RuntimeError:
            pass
        res['toppedTo'] = now().get('queueLength')
        for at in (99, 100):
            cli('seek', str(max(0.0, (now().get('duration') or 30) - 4)))
            try:
                until(lambda n: (n.get('queuePosition') or 0) >= at and n.get('state') == 'Playing', 30, f'position {at}')
            except RuntimeError:
                break
        n = now()
        res['at100'] = n.get('queuePosition') == 100
        res['item100IsSong100'] = playing(n, found['song100'])
        res['item100IsX'] = n.get('itemId') == x
        res['pages'] = snap().get('library', {}).get('songs', {}).get('pages')
        res['T'] = found['songs'] > 100 and res['clickedPlays'] and (res['toppedTo'] or 0) >= 200 and res['at100'] and res['item100IsSong100'] and (res['pages'] or 0) >= 2
        clean('topup-sabotage' if sabotage else 'topup', res)

    def shot(name, args, steps):
        png = os.path.join(PRIVATE, name + '.png')
        res = s9.run(a.binary, env, sink, out, 'shot-' + name, COMMON + ['--queue-open', '--screenshot', png, '--after', '30'] + args, lambda p, s, r: (steps(r), p.wait(60)))
        return os.path.exists(png) and not res.get('error')

    songs_click = ['--route', 'songs', '--click-row', '0']
    results = [s9.run(a.binary, env, sink, out, 'f8', COMMON, f8),
               s9.run(a.binary, env, sink, out, 'jump', COMMON + ['--jump-to', '3'], jump),
               s9.run(a.binary, env, sink, out, 'topup', COMMON + songs_click + ['--jump-to', '98'], topup),
               s9.run(a.binary, env, sink, out, 'topup-sabotage', COMMON + songs_click + ['--jump-to', '98'], lambda p, s, r: topup(p, s, r, True))]
    os.makedirs(PRIVATE, exist_ok=True)
    shots = {'f8': shot('f8', [], four), 'songs': shot('songs', songs_click, lambda r: safe())}
    return {'found': {'songs': found['songs'], 'queue': q, 'x': x}, 'results': results, 'privateShots': shots}


def synthetic(a, out, sock, sink):
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), s9.Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    try:
        with cs.isolated('s10-synthetic', False) as (iso, config):
            for f, v in (('developer-token', s6.FIXTURE_DEVELOPER), ('music-user-token', s6.FIXTURE_USER)):
                open(os.path.join(config, f), 'w').write(v)
            env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **iso)
            env.pop('DISPLAY', None)
            png = os.path.join(out, 'queue-synthetic.png')
            cmd = [a.binary, '--route', 'songs', '--queue-open', '--queue-fixture', '--screenshot', png, '--after', '6', '--api-base', base, '--developer-token-url', base + '/developer-token.json'] + COMMON
            subprocess.run(cmd, env=env, capture_output=True, timeout=60)
            ok = os.path.exists(png) and s6.png_facts(png)['distinctRows'] > 1
            if ok:
                os.makedirs(PUBLIC, exist_ok=True)
                shutil.copy(png, PUBLIC)
            return {'synthetic': ok}
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
    with cs.session(a.out, f'bramley-s10-{a.mode}') as (sock, sink, _):
        report = live(a, a.out, sock, sink) if a.mode == 'live' else synthetic(a, a.out, sock, sink)
    json.dump(report, open(os.path.join(a.out, a.mode + '.json'), 'w'), indent=1)
    print(json.dumps(report, indent=1))


if __name__ == '__main__':
    main()
