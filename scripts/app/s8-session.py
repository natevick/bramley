import argparse, contextlib, importlib.util, json, os, shutil, subprocess, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, 'scripts', 'app', file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cs, s6, s7 = load('covers_session', 'covers-session.py'), load('s6_shots', 's6-shots.py'), load('s7_session', 's7-session.py')
WINDOW, CUT_BEFORE_END = 5, 3
LONG_PLAYLIST = 'pl.f68f14135e0b4b3eb42eb5776100f064'
PLAYLIST_TERM, OUT, NAMES = 'queen essentials', os.path.join(ROOT, 'target', 's8'), os.path.join(ROOT, 'target', 'tmp', 's8', 'names.txt')
SHOTS = {'real': os.path.join(ROOT, 'target', 'screens', 'PRIVATE-real-art', 's8'), 'synthetic': os.path.join(ROOT, 'target', 'screens', 'synthetic', 's8')}
PUBLISH, SHOT = os.path.join(ROOT, 'docs', 'screens', 's8'), 'album-detail-dark-1100x720'


class Fixture(s7.Fixture):
    def do_GET(self):
        if not self.path.startswith('/v1/catalog/us/albums/'):
            return super().do_GET()
        base = f'http://127.0.0.1:{self.server.server_address[1]}'
        song = lambda i: {'id': f'9{i:03d}', 'type': 'songs', 'attributes': {'name': f'Fixture Song {i}', 'artistName': 'Fixture Artist', 'durationInMillis': 180000 + i * 7000, **({'playParams': {'id': f'9{i:03d}', 'kind': 'song'}} if i != 4 else {})}}
        album = {'id': '1000', 'type': 'albums', 'attributes': {'name': 'Fixture Album', 'artistName': 'Fixture Artist', 'trackCount': 12, 'artwork': {'url': f'{base}/art/0/{{w}}x{{h}}bb.jpg'}},
                 'relationships': {'tracks': {'data': [song(i) for i in range(1, 13)]}}}
        self.reply(json.dumps({'data': [album]}).encode())


s7.Fixture = Fixture


def launch(binary, env, sock, sink, name, extra, out):
    st = os.path.join(out, name + '.status.json')
    with contextlib.suppress(FileNotFoundError):
        os.remove(st)
    run_env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **env)
    run_env.pop('DISPLAY', None)
    cs.assert_isolated(run_env)
    rc = subprocess.run([binary, 'now'], env=run_env, capture_output=True).returncode
    if rc != 3 or not cs.sink_exists(sink):
        raise RuntimeError(f'{name}: slot held (now exit {rc}) or sink missing; refusing')
    cmd = [binary, '--status', st, '--theme', 'dark', '--size', '1100x720', '--output-device', sink] + extra
    p = subprocess.Popen(cmd, env=run_env, stdout=open(os.path.join(out, name + '.log'), 'w'), stderr=subprocess.STDOUT, start_new_session=True)
    guard = cs.Guard(p.pid, sink)
    guard.start()
    return p, guard, st, run_env


def cli(binary, env, *args):
    return subprocess.run([binary, *args], env=env, capture_output=True, text=True, timeout=15)


def note_names(binary, env, *args):
    try:
        reply = json.loads(cli(binary, env, *args).stdout.splitlines()[0])
    except (IndexError, ValueError):
        return
    found = reply.get('results', {})
    rows = [reply] + [i for page in found.values() if isinstance(page, dict) for i in page.get('items', [])] if isinstance(found, dict) else [reply]
    names = {n for r in rows for n in (r.get('title'), r.get('name'), r.get('artist'), r.get('album')) if isinstance(n, str) and len(n) > 3}
    with os.fdopen(os.open(NAMES, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600), 'a') as f:
        f.writelines(n + '\n' for n in sorted(names))


detail = lambda s: (s or {}).get('detail') or {}
playing = lambda s, i: s.get('state') == 'Playing' and detail(s).get('trackIds', [])[i:i + 1] == [s.get('itemId')]


def advancing(st):
    a = cs.status(st) or {}
    time.sleep(3)
    b = cs.status(st) or {}
    return round((b.get('position') or 0) - (a.get('position') or 0), 2)


def ran(s, guard, sink):
    return {'radeonsi': 'radeonsi' in str((s or {}).get('glRenderer')), 'frames': (s or {}).get('framesPainted', 0) > 0,
            'nullSinkOnly': sorted(map(str, guard.seen)) == [sink], 'guardKilled': guard.killed}


def transition(binary, env, st, at, cut, start):
    s, _ = s7.poll(st, lambda s: playing(s, at) and (s.get('duration') or 0) > 30 and (s.get('position') or 0) > start + 2, 30)
    if not s or not playing(s, at):
        return {'reached': False}
    ids, duration, sent_seq = detail(s)['trackIds'], s['duration'], s.get('seq', 0)
    target = round(duration - 20, 1)
    cli(binary, env, 'seek', str(target))
    base, land, last, flip, samples, sent_next = None, None, start, None, [], False
    end = time.time() + 40
    while time.time() < end and (flip is None or time.time() < flip[0] + WINDOW):
        s, now = cs.status(st) or {}, time.time()
        pos = s.get('position') or 0
        samples.append((now, pos))
        if base is None and s.get('seq', 0) > sent_seq and s.get('applied', 0) >= s['seq']:
            base = {k: s[k] for k in ('drainsAcked', 'drainTimeouts', 'sentMs', 'skippedMs')}
            land = pos
        if flip is None and s.get('itemId') == ids[at + 1] and base:
            flip = (now, {k: s[k] - base[k] for k in base}, pos)
        if flip and base:
            after = {k: s[k] - base[k] for k in base}
        if flip is None and s.get('itemId') == ids[at] and base:
            last = max(last, pos)
            if cut and not sent_next and last - start >= duration - CUT_BEFORE_END:
                cli(binary, env, 'next')
                sent_next = True
        time.sleep(0.02)
    if flip is None:
        return {'advanced': False, 'pass': False}
    window = [(t, p) for t, p in samples if abs(t - flip[0]) <= WINDOW]
    stall, moved = 0.0, window[0][0]
    for (t0, p0), (t1, p1) in zip(window, window[1:]):
        moved = t1 if p1 > p0 else moved
        stall = max(stall, t1 - moved)
    d = flip[1]
    res = {'acked': d['drainsAcked'], 'timeouts': d['drainTimeouts'], 'sentS': after['sentMs'] / 1000, 'skippedS': after['skippedMs'] / 1000, 'skippedAtFlipS': d['skippedMs'] / 1000,
           'stallS': round(stall, 2), 'landedOff': round(land - start - target, 2), 'tailShortBy': round(duration - (last - start), 2), 'nextStart': flip[2]}
    res['literal'] = res['acked'] >= 1 and res['timeouts'] == 0
    res['measured'] = res['sentS'] >= (duration - target) - 3 and res['skippedS'] <= 0.1 and stall <= 1
    res['pass'] = abs(res['landedOff']) <= 1 and (res['literal'] or res['measured'])
    return res


def album_run(binary, env, sock, sink, out, name, sabotage):
    for k in range(10):
        p, guard, st, renv = launch(binary, env, sock, sink, f'{name}-{k}', ['--click-cover', str(k), '--click-track', '3'], out)
        try:
            s, _ = s7.poll(st, lambda s: detail(s).get('state') == 'error' or detail(s).get('complete'), 30)
            if detail(s).get('tracks', 0) < 6:
                continue
            loaded = time.time()
            s, _ = s7.poll(st, lambda s: playing(s, 2), 10)
            res = {'cover': k, 'ac': bool(s and playing(s, 2)), 'acSeconds': round(time.time() - loaded, 2), 'transitions': []}
            note_names(binary, renv, 'now')
            start = 0.0
            for i in range(3):
                t = transition(binary, renv, st, 2 + i, sabotage and i == 2, start)
                res['transitions'].append(t)
                start = t.get('nextStart', start)
            note_names(binary, renv, 'now')
            res.update(ran(cs.status(st), guard, sink))
            clean = res['nullSinkOnly'] and not res['guardKilled']
            res['f6'] = 'PASS' if all(t.get('pass') for t in res['transitions']) and clean else 'FAIL'
            return res
        finally:
            s7.finish(p, guard)
    return {'f6': 'FAIL', 'why': 'no album with 6 tracks in the first 10 covers'}


def click_run(binary, env, sock, sink, out, name, extra, index, route):
    p, guard, st, renv = launch(binary, env, sock, sink, name, extra, out)
    try:
        s, _ = s7.poll(st, lambda s: s.get('route') == route and playing(s, index), 40)
        res = {'name': name, 'route': (s or {}).get('route'), 'tracks': detail(s).get('tracks'), 'art': detail(s).get('art'), 'clickedPlays': bool(s and playing(s, index)),
               'searchPlaylists': ((s or {}).get('search') or {}).get('playlists')}
        res['advanced'] = advancing(st)
        note_names(binary, renv, 'now')
        if '--search' in extra:
            note_names(binary, renv, 'search', extra[extra.index('--search') + 1])
        res.update(ran(cs.status(st), guard, sink))
        res['pass'] = res['clickedPlays'] and res['advanced'] >= 2 and res['radeonsi'] and res['nullSinkOnly'] and not res['guardKilled']
        return res
    finally:
        s7.finish(p, guard)


def sequence(binary, env, sock, sink, out, name, extra, clicks):
    p, guard, st, renv = launch(binary, env, sock, sink, name, extra + ['--click-track', ','.join(map(str, clicks))], out)
    try:
        rows, end = [], time.time() + 12 * len(clicks) + 20
        while time.time() < end:
            s = cs.status(st) or {}
            ids, item = detail(s).get('trackIds', []), s.get('itemId')
            if s.get('state') == 'Playing' and item in ids and (not rows or rows[-1] != ids.index(item) + 1):
                rows.append(ids.index(item) + 1)
            time.sleep(0.1)
        note_names(binary, renv, 'now')
        return {'clicks': clicks, 'rowsPlayed': rows, **ran(cs.status(st), guard, sink)}
    finally:
        s7.finish(p, guard)


def leak_grep():
    names = [n for n in open(NAMES).read().splitlines() if n]
    corpus = subprocess.run(['git', 'diff', 'main'], cwd=ROOT, capture_output=True, text=True).stdout
    corpus += subprocess.run(['git', 'log', 'main..HEAD', '--format=%B'], cwd=ROOT, capture_output=True, text=True).stdout
    for d, _, files in os.walk(OUT):
        corpus += ''.join(open(os.path.join(d, f), errors='replace').read() for f in files if f.endswith('.log'))
    count = lambda text: sum(text.lower().count(n.lower()) for n in names)
    hits = sorted({n for n in names if n.lower() in corpus.lower()})
    print('leak-grep hits (stdout only):', hits, flush=True)
    return {'names': len(names), 'hits': len(hits), 'plantRaises': bool(names) and count(corpus + names[-1]) > count(corpus)}


def live(binary, out):
    os.makedirs(os.path.dirname(NAMES), exist_ok=True)
    try:
        with cs.session(out, 'bramley-s8-live') as (sock, sink, _), cs.isolated('s8-live', True) as (env, _):
            runs = {'l1': click_run(binary, env, sock, sink, out, 'l1', ['--click-cover', '0', '--click-track', '1'], 0, 'Album')}
            runs['l1']['pass'] = runs['l1']['pass'] and runs['l1']['art'] is True
            runs['l3'] = click_run(binary, env, sock, sink, out, 'l3', ['--route', 'search', '--search', PLAYLIST_TERM, '--click-first-playlist', '--click-track', '5'], 4, 'Playlist')
            runs['l5'] = click_run(binary, env, sock, sink, out, 'l5', ['--route', 'playlist:' + LONG_PLAYLIST, '--click-track', '105'], 104, 'Playlist')
            runs['l2'] = album_run(binary, env, sock, sink, out, 'l2', False)
            runs['l2']['pass'] = runs['l2']['f6'] == 'PASS' and runs['l2'].get('ac') and runs['l2'].get('acSeconds', 99) <= 10
            runs['f6s'] = album_run(binary, env, sock, sink, out, 'f6s', True)
            runs['shuffleThen3'] = sequence(binary, env, sock, sink, out, 'shuffle-then-3', ['--click-cover', '0'], [0, 3])
            runs['play5ThenShuffle'] = sequence(binary, env, sock, sink, out, 'play5-then-shuffle', ['--click-cover', '0'], [5, 0])
        runs['leakGrep'] = leak_grep()
    finally:
        if os.path.exists(NAMES):
            os.remove(NAMES)
    for name, r in runs.items():
        print(name, json.dumps(r), flush=True)
    verdict = {'l1': runs['l1']['pass'], 'l2': runs['l2']['pass'], 'l3': runs['l3']['pass'], 'l5': runs['l5']['pass'], 'f6SabotageFails': runs['f6s']['f6'] == 'FAIL', 'runs': runs}
    json.dump(verdict, open(os.path.join(out, 'live.verdict.json'), 'w'), indent=1)
    return all(verdict[k] for k in ('l1', 'l2', 'l3', 'l5', 'f6SabotageFails'))


def shot(binary, which, stamp):
    root, other = SHOTS[which], SHOTS['synthetic' if which == 'real' else 'real']
    if os.path.commonpath([root, os.path.join(ROOT, 'docs')]) == os.path.join(ROOT, 'docs') or os.path.commonpath([root, other]) in (root, other):
        raise SystemExit(f'refusing: {root}')
    out = os.path.join(root, stamp)
    os.makedirs(out)
    open(os.path.join(root, s6.MARK[which]), 'w').close()
    views = {'album': ['--click-cover', '0']} if which == 'synthetic' else {'album': ['--click-cover', '0'], 'playlist': ['--route', 'search', '--search', PLAYLIST_TERM, '--click-first-playlist']}
    ok = True
    with cs.session(out, f'bramley-s8-{which}') as (sock, sink, _), cs.isolated(f's8-{which}', which == 'real') as (env, config), contextlib.ExitStack() as stack:
        extra = []
        if which == 'synthetic':
            base, token_url = stack.enter_context(s7.fixture(config))
            extra = ['--api-base', base] + token_url
        for view, flags in views.items():
            png = os.path.join(out, f'{view}-detail-dark-1100x720.png')
            p, guard, st, _ = launch(binary, env, sock, sink, view, extra + flags + ['--screenshot', png, '--after', '12'], out)
            try:
                exited = p.wait(60)
            finally:
                s7.finish(p, guard)
            s = cs.status(st) or {}
            res = {'view': view, 'exit': exited, 'detail': {k: detail(s).get(k) for k in ('state', 'tracks', 'art')}, 'engineStarted': s.get('engineStarted'), **(s6.png_facts(png) if os.path.exists(png) else {'size': None})}
            res['ok'] = exited == 0 and 'radeonsi' in str(s.get('glRenderer')) and detail(s).get('art') is True and res['size'] == [1100, 720] and not guard.seen
            print(json.dumps(res), flush=True)
            ok &= res['ok']
            if which == 'synthetic' and res['ok']:
                shutil.rmtree(PUBLISH, ignore_errors=True)
                os.makedirs(PUBLISH)
                shutil.copy(png, os.path.join(PUBLISH, SHOT + '.png'))
    return ok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('mode', choices=('live', 'real', 'synthetic'))
    ap.add_argument('binary')
    a = ap.parse_args()
    if subprocess.run(['python3', '-I', os.path.join(ROOT, 'scripts', 'acceptance', 'gate.py'), '--preflight']).returncode:
        raise SystemExit('refusing: the box is not quiet')
    stamp = time.strftime('%Y%m%d-%H%M%S')
    if a.mode != 'live':
        raise SystemExit(0 if shot(a.binary, a.mode, stamp) else 1)
    out = os.path.join(OUT, 'live-' + stamp)
    os.makedirs(out)
    raise SystemExit(0 if live(a.binary, out) else 1)


if __name__ == '__main__':
    main()
