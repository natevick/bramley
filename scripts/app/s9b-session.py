import argparse, http.server, importlib.util, json, os, re, shutil, statistics, subprocess, threading, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
spec = importlib.util.spec_from_file_location('s9_session', os.path.join(ROOT, 'scripts', 'app', 's9-session.py'))
s9 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s9)
cs, s6 = s9.cs, s9.s6
SCRATCH = os.path.join(ROOT, 'target', 'tmp', 's9b')
NAMES = os.path.join(SCRATCH, 'names.txt')
PRIVATE = os.path.join(ROOT, 'target', 'screens', 'PRIVATE-real-art', 's9b')
PUBLIC = os.path.join(ROOT, 'docs', 'screens', 's9b')
GENERIC = {'Home', 'Back', 'Untitled'}


def discover(config):
    tokens = [open(os.path.join(config, f)).read().strip() for f in ('developer-token', 'music-user-token')]
    artists, total = s9.every('/v1/me/library/artists', tokens)
    sizes = {a['id']: len(s9.api(f"/v1/me/library/artists/{a['id']}/albums?limit=100", tokens)['data']) for a in artists[:40]}
    most = max(sizes, key=sizes.get)
    albums, _ = s9.every(f'/v1/me/library/artists/{most}/albums', tokens)
    tracks = [t for a in albums for t in s9.every(f"/v1/me/library/albums/{a['id']}/tracks", tokens)[0]]
    name = next(a['attributes'].get('name') for a in artists if a['id'] == most)
    names = {r.get('attributes', {}).get(k) for r in artists + albums + tracks for k in ('name', 'artistName', 'albumName')}
    names = sorted(n for n in names if n and len(n) >= 4 and n not in GENERIC)
    os.makedirs(SCRATCH, exist_ok=True)
    with os.fdopen(os.open(NAMES, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), 'w') as f:
        f.write('\n'.join(names) + '\n')
    first = tracks[0]
    played = {first['id'], first.get('attributes', {}).get('playParams', {}).get('catalogId')} - {None}
    others = sum(t.get('attributes', {}).get('artistName') != name for t in tracks)
    found = {'artistsTotal': total, 'albums': len(albums), 'tracks': len(tracks), 'tracksByOtherArtists': others, 'names': len(names)}
    return found, played, names, most


def live(a, out, sock, sink):
    try:
        with cs.isolated('s9b-live', True) as (iso, config):
            found, played, names, most = discover(config)
            results = live_runs(a, out, sink, sock, iso, found, played, most)
            found.update(s9.leak_grep(names, out))
            return found, results
    finally:
        if os.path.exists(NAMES):
            os.remove(NAMES)


def live_runs(a, out, sink, sock, iso, found, played, most):
    env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **iso)
    env.pop('DISPLAY', None)
    cs.assert_isolated(env)
    lib = lambda snap: snap().get('library', {})

    def artists(p, snap, res, scroll=True):
        cs.wait_for(lambda: lib(snap).get('artists', {}).get('pages', 0) > 0, 30, 'the first artists page')
        if not scroll:
            res['r2ArtistsPss'] = round(statistics.median(s9.pss(p, 20)), 1)
        try:
            cs.wait_for(lambda: snap().get('route') == 'Artist', 120 if scroll else 60, 'the artist view')
        except RuntimeError:
            pass
        shelf = lib(snap).get('artists', {})
        res['A1'] = snap().get('route') == 'Artist' and shelf.get('loaded') == shelf.get('total') == found['artistsTotal'] and (found['artistsTotal'] <= 100 or shelf.get('pages', 0) > 1)

    def artist(p, snap, res):
        if not cs.sink_exists(sink) or cs.default_sink() in (None, sink):
            raise RuntimeError('audio not safe before play')
        cs.wait_for(lambda: snap().get('state') == 'Playing' and (snap().get('position') or 0) > 0, 30, 'playing')
        cs.wait_for(lambda: snap().get('itemId'), 10, 'a now-playing item')
        res['clickedIsPlaying'] = snap().get('itemId') in played
        title = json.loads(subprocess.run([a.binary, 'now'], env=env, capture_output=True, text=True).stdout or '{}').get('title')
        res['nowHasTitle'] = bool(title)
        res['titleInStatusFile'] = bool(title) and title in open(os.path.join(out, 'artist.status.json')).read()
        done = lambda: lib(snap).get('artistSongs', {}).get('more') is False
        try:
            cs.wait_for(done, 120, 'every album read')
        except RuntimeError:
            pass
        view = lib(snap)
        songs = view.get('artistSongs', {})
        res['view'] = {'albums': view.get('artistAlbums', {}).get('loaded'), 'songs': songs.get('loaded'), 'skipped': songs.get('skipped')}
        counted = songs.get('loaded') == found['tracks'] or (songs.get('skipped', 0) > 0 and songs.get('loaded', 0) < found['tracks'])
        res['A2'] = res['view']['albums'] == found['albums'] and done() and counted and 0 < view.get('artTextures', 0) <= s9.ART_CAP and view.get('rowsBuiltMax', 99) <= s9.ROWS_BAR
        res['r1ArtistPss'] = round(statistics.median(s9.pss(p, 20)), 1)
        res['A3'] = res['clickedIsPlaying'] and snap().get('state') == 'Playing' and res['nowHasTitle'] and not res['titleInStatusFile']

    common = ['--size', '1100x720', '--route', 'artists', '--click-last']
    results = [s9.run(a.binary, env, sink, out, 'artists', common + ['--scroll-end', '6'], artists),
               s9.run(a.binary, env, sink, out, 'sabotage-s1', common, lambda p, s, r: artists(p, s, r, False)),
               s9.run(a.binary, env, sink, out, 'artist', ['--size', '1100x720', '--route', f'artist:{most}', '--scroll-end', '6', '--click-row', '0'], artist)]
    os.makedirs(PRIVATE, exist_ok=True)
    for route in ('artists', f'artist:{most}'):
        png = os.path.join(PRIVATE, route.split(':')[0] + '.png')
        subprocess.run([a.binary, '--route', route, '--screenshot', png, '--after', '15', '--size', '1100x720'], env=env, capture_output=True, timeout=60)
    found['privateShots'] = sorted(os.listdir(PRIVATE))
    return results


class Fixture(s9.Fixture):
    def do_GET(self):
        base = f'http://127.0.0.1:{self.server.server_address[1]}'
        path = self.path.split('?')[0]
        if m := re.fullmatch(r'/v1/me/library/artists(?:/r\.(\d+))?', path):
            ids = [int(m[1])] if m[1] else range(30)
            rows = [{'id': f'r.{i}', 'type': 'library-artists', 'attributes': {'name': f'Synthetic Artist {i + 1}'}} for i in ids]
        elif re.fullmatch(r'/v1/me/library/artists/r\.\d+/albums', path):
            rows = [{'id': f'l.{i}', 'type': 'library-albums', 'attributes': {'name': f'Synthetic Album {i + 1}', 'artwork': {'url': f'{base}/art/{i}/{{w}}x{{h}}bb.jpg'}}} for i in range(9)]
        elif m := re.fullmatch(r'/v1/me/library/albums/l\.(\d+)/tracks', path):
            rows = [{'id': f'i.{m[1]}.{i}', 'type': 'library-songs', 'attributes': {'name': f'Synthetic Song {i + 1}', 'artistName': 'Synthetic Artist 1', 'artwork': {'url': f"{base}/art/{m[1]}/{{w}}x{{h}}bb.jpg"}}} for i in range(4)]
        else:
            return super().do_GET()
        body = json.dumps({'data': rows, 'meta': {'total': len(rows)}}).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def synthetic(a, out, sock, sink):
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    try:
        with cs.isolated('s9b-synthetic', False) as (iso, config):
            for f, v in (('developer-token', s6.FIXTURE_DEVELOPER), ('music-user-token', s6.FIXTURE_USER)):
                open(os.path.join(config, f), 'w').write(v)
            env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **iso)
            env.pop('DISPLAY', None)
            report = {}
            for name, route in (('artists', 'artists'), ('artist', 'artist:r.0')):
                png, st = os.path.join(out, f'{name}-synthetic.png'), os.path.join(out, f'{name}-synthetic.status.json')
                cmd = [a.binary, '--status', st, '--route', route, '--screenshot', png, '--after', '6', '--size', '1100x720', '--api-base', base, '--developer-token-url', base + '/developer-token.json']
                subprocess.run(cmd, env=env, capture_output=True, timeout=60)
                lib = (cs.status(st) or {}).get('library', {})
                report[name] = {'png': os.path.exists(png), 'artTextures': lib.get('artTextures', 0), 'artists': lib.get('artists', {}).get('loaded'), 'songs': lib.get('artistSongs', {}).get('loaded')}
            if report['artist']['png'] and report['artist']['artTextures'] > 0:
                os.makedirs(PUBLIC, exist_ok=True)
                shutil.copy(os.path.join(out, 'artist-synthetic.png'), PUBLIC)
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
    with cs.session(a.out, f'bramley-s9b-{a.mode}') as (sock, sink, _):
        report = live(a, a.out, sock, sink) if a.mode == 'live' else synthetic(a, a.out, sock, sink)
    json.dump(report, open(os.path.join(a.out, a.mode + '.json'), 'w'), indent=1)
    print(json.dumps(report, indent=1))


if __name__ == '__main__':
    main()
