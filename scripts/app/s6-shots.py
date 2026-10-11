import argparse, colorsys, http.server, importlib.util, json, os, re, shutil, subprocess, threading, time, zlib

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
_spec = importlib.util.spec_from_file_location('covers_session', os.path.join(ROOT, 'scripts', 'app', 'covers-session.py'))
cs = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cs)

SHOTS = [(route, queue, theme, '1100x720') for route in ('home', 'search', 'playlists') for queue in (False, True) for theme in ('dark', 'light')]
SHOTS += [('home', queue, theme, '800x600') for queue in (False, True) for theme in ('dark', 'light')]
PUBLIC = ('home-noqueue-dark-1100x720', 'home-queue-light-1100x720', 'search-queue-dark-1100x720')
SCREENS, DOCS = os.path.join(ROOT, 'target', 'screens'), os.path.join(ROOT, 'docs')
OUT = {'real': os.path.join(SCREENS, 'PRIVATE-real-art'), 'synthetic': os.path.join(SCREENS, 'synthetic')}
PUBLISH = os.path.join(DOCS, 'screens', 's6')
MARK = {'real': 'REAL-ART-NEVER-PUBLISH', 'synthetic': 'SYNTHETIC-ONLY'}
FIXTURE_DEVELOPER, FIXTURE_USER = 's6-fixture-developer-token', 's6-fixture-user-token'
ART = os.path.join(ROOT, 'target', 'tmp', 's6-art')


def art(index, size):
    path = os.path.join(ART, f'flat-{index}-{size}.jpg')
    if not os.path.exists(path):
        os.makedirs(ART, exist_ok=True)
        hue = lambda h: '0x%02x%02x%02x' % tuple(int(c * 255) for c in colorsys.hsv_to_rgb(h % 1.0, 0.65, 0.85))
        source = f'color=c={hue(index / 50)}:s={size}x{size}'
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-f', 'lavfi', '-i', source, '-frames:v', '1', path], check=True)
    return open(path, 'rb').read()


class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        base = f'http://127.0.0.1:{self.server.server_address[1]}'
        found = re.fullmatch(r'/art/(\d+)/(\d+)x\d+bb\.jpg', self.path)
        if self.path == '/developer-token.json':
            body = json.dumps({'token': FIXTURE_DEVELOPER, 'exp': int(time.time()) + 90 * 86400}).encode()
        elif self.path == '/v1/me/storefront':
            body = b'{"data":[{"id":"us"}]}'
        elif self.path.startswith('/v1/catalog/us/charts'):
            albums = [{'attributes': {'artwork': {'url': f'{base}/art/{i}/{{w}}x{{h}}bb.jpg'}}} for i in range(50)]
            body = json.dumps({'results': {'albums': [{'data': albums}]}}).encode()
        elif found:
            body = art(int(found[1]), int(found[2]))
        else:
            self.send_response(404)
            self.end_headers()
            return
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def png_facts(path):
    data = open(path, 'rb').read()
    w, h = int.from_bytes(data[16:20], 'big'), int.from_bytes(data[20:24], 'big')
    idat, at = b'', 8
    while at < len(data):
        n, kind = int.from_bytes(data[at:at + 4], 'big'), data[at + 4:at + 8]
        idat += data[at + 8:at + 8 + n] if kind == b'IDAT' else b''
        at += 12 + n
    raw, stride = zlib.decompress(idat), 1 + w * 4
    rows = {raw[i * stride + 1:(i + 1) * stride] for i in range(h)}
    return {'size': [w, h], 'distinctRows': len(rows)}


def shoot(binary, env, sock, sink, name, shot, extra, out, after):
    route, queue, theme, size = shot
    png, st = os.path.join(out, name + '.png'), os.path.join(out, name + '.status.json')
    cmd = [binary, '--status', st, '--screenshot', png, '--after', str(after), '--route', route, '--theme', theme, '--size', size] + (['--queue-open'] if queue else []) + extra
    run_env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **env)
    run_env.pop('DISPLAY', None)
    cs.assert_isolated(run_env)
    p = subprocess.Popen(cmd, env=run_env, stdout=open(os.path.join(out, name + '.log'), 'w'), stderr=subprocess.STDOUT, start_new_session=True)
    guard = cs.Guard(p.pid, sink)
    guard.start()
    try:
        exited = p.wait(after + 40)
    except subprocess.TimeoutExpired:
        p.kill()
        exited = 'timeout'
    guard.stop = True
    s = cs.status(st) or {}
    res = {'name': name, 'exit': exited, 'guardKilled': guard.killed, 'streamsSeen': sorted(map(str, guard.seen)), **{k: s.get(k) for k in ('route', 'queueOpen', 'glRenderer', 'texturesUploaded', 'coversFailed', 'engineStarted', 'banner')}}
    res.update(png_facts(png) if os.path.exists(png) else {'size': None})
    want = [int(n) for n in size.split('x')]
    res['ok'] = (exited == 0 and not guard.killed and not guard.seen and 'radeonsi' in str(s.get('glRenderer')) and s.get('engineStarted') is False
                 and s.get('route', '').lower() == route and s.get('queueOpen') == queue and s.get('texturesUploaded') == 50
                 and res['size'] == want and res.get('distinctRows', 0) > 1)
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('set', choices=('real', 'synthetic'))
    ap.add_argument('binary')
    a = ap.parse_args()
    other_set = 'synthetic' if a.set == 'real' else 'real'
    out, other = OUT[a.set], OUT[other_set]
    if os.path.exists(os.path.join(out, MARK[other_set])) or os.path.commonpath([out, DOCS]) == DOCS or os.path.commonpath([out, other]) in (out, other):
        raise SystemExit(f'refusing: {out} holds the other set or is publishable')
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    open(os.path.join(out, MARK[a.set]), 'w').close()
    server, results = None, []
    with cs.session(out, f'bramley-s6-{a.set}') as (sock, sink, _), cs.isolated(f's6-{a.set}', a.set == 'real') as (env, config):
        extra, after = [], 14
        if a.set == 'synthetic':
            server = type('Server', (http.server.ThreadingHTTPServer,), {'request_queue_size': 128})(('127.0.0.1', 0), Fixture)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            base = f'http://127.0.0.1:{server.server_address[1]}'
            for f, v in (('developer-token', FIXTURE_DEVELOPER), ('music-user-token', FIXTURE_USER)):
                open(os.path.join(config, f), 'w').write(v)
            extra, after = ['--api-base', base, '--developer-token-url', base + '/developer-token.json'], 6
        try:
            for shot in SHOTS:
                route, queue, theme, size = shot
                name = f"{route}-{'queue' if queue else 'noqueue'}-{theme}-{size}"
                if a.set == 'synthetic' and name not in PUBLIC:
                    continue
                results.append(shoot(a.binary, env, sock, sink, name, shot, extra, out, after))
                print(json.dumps(results[-1]), flush=True)
        finally:
            if server:
                server.shutdown()
    json.dump(results, open(os.path.join(out, 'shots.json'), 'w'), indent=1)
    bad = [r['name'] for r in results if not r['ok']]
    print(f'{len(results) - len(bad)}/{len(results)} shots ok' + (f'; failed: {bad}' if bad else ''))
    if a.set == 'synthetic' and not bad and not os.path.exists(os.path.join(out, MARK['real'])):
        shutil.rmtree(PUBLISH, ignore_errors=True)
        os.makedirs(PUBLISH)
        for name in PUBLIC:
            shutil.copy(os.path.join(out, name + '.png'), PUBLISH)
    raise SystemExit(1 if bad else 0)


if __name__ == '__main__':
    main()
