import argparse, importlib.util, json, os, shutil, statistics, subprocess, threading, time, http.server

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'scripts', 'app', file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cs, s6 = load('covers_session', 'covers-session.py'), load('s6_shots', 's6-shots.py')
TERM, OUT = 'bohemian rhapsody', os.path.join(ROOT, 'target', 's7')
SHOTS = {'real': os.path.join(ROOT, 'target', 'screens', 'PRIVATE-real-art', 's7'), 'synthetic': os.path.join(ROOT, 'target', 'screens', 'synthetic', 's7')}
PUBLISH, SHOT = os.path.join(ROOT, 'docs', 'screens', 's7'), 'search-results-dark-1100x720'


class Fixture(s6.Fixture):
    def do_GET(self):
        if self.path == '/v1/me/storefront':
            return self.reply(b'{"data":[{"id":"us","type":"storefronts"}]}')
        if not self.path.startswith('/v1/catalog/us/search'):
            return super().do_GET()
        base = f'http://127.0.0.1:{self.server.server_address[1]}'
        def rows(kind, names, art=True):
            attrs = lambda i, n: {'name': n, **({'artistName': 'Fixture Artist'} if kind != 'artists' else {}), **({'artwork': {'url': f'{base}/art/{i * 7}/{{w}}x{{h}}bb.jpg'}} if art else {})}
            return {'data': [{'id': f'{kind[0]}{i}', 'type': kind, 'attributes': attrs(i, n)} for i, n in enumerate(names)]}
        results = {'songs': rows('songs', [f'Fixture Song {i}' for i in range(1, 5)]), 'albums': rows('albums', ['Fixture Album A', 'Fixture Album B']),
                   'artists': rows('artists', ['Fixture Artist'], art=False), 'playlists': rows('playlists', ['Fixture Playlist'])}
        self.reply(json.dumps({'results': results}).encode())

    def reply(self, body):
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def launch(binary, env, sock, sink, name, extra, out):
    st = os.path.join(out, name + '.status.json')
    run_env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **env)
    run_env.pop('DISPLAY', None)
    cs.assert_isolated(run_env)
    rc = subprocess.run([binary, 'now'], env=run_env, capture_output=True).returncode
    if rc != 3 or not cs.sink_exists(sink):
        raise RuntimeError(f'{name}: slot held (now exit {rc}) or sink missing; refusing')
    cmd = [binary, '--status', st, '--route', 'search', '--theme', 'dark', '--size', '1100x720'] + extra
    p = subprocess.Popen(cmd, env=run_env, stdout=open(os.path.join(out, name + '.log'), 'w'), stderr=subprocess.STDOUT, start_new_session=True)
    guard = cs.Guard(p.pid, sink)
    guard.start()
    return p, guard, st


def finish(p, guard):
    guard.stop = True
    if p.poll() is None:
        p.terminate()
        try:
            p.wait(10)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()


def poll(st, until, timeout):
    first, end, seen = None, time.time() + timeout, None
    while time.time() < end:
        s = cs.status(st) or {}
        if first is None and s.get('framesPainted', 0) >= 1:
            first = time.time()
        if first and until(s):
            return s, round((time.time() - first) * 1000)
        seen = s or seen
        time.sleep(0.02)
    return seen, None


def search_run(binary, env, sock, sink, name, extra, out):
    p, guard, st = launch(binary, env, sock, sink, name, ['--search', TERM] + extra, out)
    try:
        s, wall = poll(st, lambda s: s.get('search', {}).get('state') in ('results', 'empty', 'error'), 15)
        s = s or {}
        res = {'name': name, 'wallMs': wall, 'search': s.get('search'), 'engineStarted': s.get('engineStarted'), 'glRenderer': s.get('glRenderer')}
        if '--click-first-song' in extra:
            playing = lambda s: s.get('state') == 'Playing' and (s.get('position') or 0) > 0
            a, _ = poll(st, playing, 15)
            time.sleep(3)
            b = cs.status(st) or {}
            res.update(itemId=b.get('itemId'), title=b.get('title'), state=b.get('state'), advanced=round((b.get('position') or 0) - ((a or {}).get('position') or 0), 2))
        res.update(guardKilled=guard.killed, streamsSeen=sorted(map(str, guard.seen)), guardPolls=guard.polls)
        return res
    finally:
        finish(p, guard)


def live(binary, out):
    results = []
    with cs.session(out, 'bramley-s7-live') as (sock, sink, _), cs.isolated('s7-live', True) as (env, _):
        for i in range(5):
            results.append(search_run(binary, env, sock, sink, f'f3-{i}', [], out))
            print(json.dumps(results[-1]), flush=True)
        results.append(search_run(binary, env, sock, sink, 'play', ['--click-first-song'], out))
        print(json.dumps(results[-1]), flush=True)
        results.append(search_run(binary, env, sock, sink, 'sabotage', ['--api-base', 'http://127.0.0.1:9'], out))
        print(json.dumps(results[-1]), flush=True)
        sink_name = sink
    f3, play, sabotage = results[:5], results[5], results[6]
    walls = [r['wallMs'] for r in f3]
    found = lambda r: (r['search'] or {}).get('state') == 'results' and (r['search'] or {}).get('results', 0) >= 2
    f3_ok = all(w is not None for w in walls) and statistics.median(walls) <= 2000 and all(found(r) and r['engineStarted'] is False and not r['streamsSeen'] and not r['guardKilled'] for r in f3)
    play_ok = play.get('state') == 'Playing' and play.get('advanced', 0) >= 2 and play['streamsSeen'] == [sink_name] and not play['guardKilled']
    sabotage_ok = (sabotage['search'] or {}).get('state') == 'error'
    verdict = {'f3': 'PASS' if f3_ok else 'FAIL', 'f3MedianWallMs': statistics.median(w for w in walls if w is not None) if any(walls) else None,
               'f3InAppMs': [(r['search'] or {}).get('ms') for r in f3], 'play': 'PASS' if play_ok else ('VOID' if not play['streamsSeen'] else 'FAIL'),
               'sabotageFailsF3': sabotage_ok, 'runs': results}
    json.dump(verdict, open(os.path.join(out, 'live.verdict.json'), 'w'), indent=1)
    print(json.dumps({k: v for k, v in verdict.items() if k != 'runs'}))
    return f3_ok and play_ok and sabotage_ok


def shot(binary, which):
    out, other = SHOTS[which], SHOTS['synthetic' if which == 'real' else 'real']
    if os.path.commonpath([out, os.path.join(ROOT, 'docs')]) == os.path.join(ROOT, 'docs') or os.path.commonpath([out, other]) in (out, other):
        raise SystemExit(f'refusing: {out}')
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    open(os.path.join(out, s6.MARK[which]), 'w').close()
    server, png = None, os.path.join(out, SHOT + '.png')
    with cs.session(out, f'bramley-s7-{which}') as (sock, sink, _), cs.isolated(f's7-{which}', which == 'real') as (env, config):
        extra, term = ['--screenshot', png, '--after', '9'], TERM
        if which == 'synthetic':
            server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            base = f'http://127.0.0.1:{server.server_address[1]}'
            for f, v in (('developer-token', s6.FIXTURE_DEVELOPER), ('music-user-token', s6.FIXTURE_USER)):
                open(os.path.join(config, f), 'w').write(v)
            extra += ['--api-base', base, '--developer-token-url', base + '/developer-token.json']
            term = 'fixture'
        p, guard, st = launch(binary, env, sock, sink, SHOT, extra + ['--search', term], out)
        try:
            exited = p.wait(60)
        finally:
            finish(p, guard)
            if server:
                server.shutdown()
    s = cs.status(st) or {}
    res = {'exit': exited, 'search': s.get('search'), 'glRenderer': s.get('glRenderer'), 'engineStarted': s.get('engineStarted'), 'streamsSeen': sorted(map(str, guard.seen)), **(s6.png_facts(png) if os.path.exists(png) else {'size': None})}
    res['ok'] = exited == 0 and 'radeonsi' in str(s.get('glRenderer')) and (s.get('search') or {}).get('art', 0) > 0 and res['size'] == [1100, 720] and not guard.seen
    json.dump(res, open(os.path.join(out, 'shot.json'), 'w'), indent=1)
    print(json.dumps(res))
    if which == 'synthetic' and res['ok']:
        shutil.rmtree(PUBLISH, ignore_errors=True)
        os.makedirs(PUBLISH)
        shutil.copy(png, PUBLISH)
    return res['ok']


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('mode', choices=('live', 'real', 'synthetic'))
    ap.add_argument('binary')
    a = ap.parse_args()
    if subprocess.run(['python3', '-I', os.path.join(ROOT, 'scripts', 'acceptance', 'gate.py'), '--preflight']).returncode:
        raise SystemExit('refusing: the box is not quiet')
    out = os.path.join(OUT, a.mode + time.strftime('-%H%M%S'))
    os.makedirs(out)
    ok = live(a.binary, out) if a.mode == 'live' else shot(a.binary, a.mode)
    raise SystemExit(0 if ok else 1)


if __name__ == '__main__':
    main()
