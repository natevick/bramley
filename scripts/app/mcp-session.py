import argparse, importlib.util, json, os, queue, shutil, signal, statistics, subprocess, threading, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
_spec = importlib.util.spec_from_file_location('covers_session', os.path.join(ROOT, 'scripts', 'app', 'covers-session.py'))
cs = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cs)

SONG, QUEUED = '6781024437', '6781079518'
TOOLS = 'search play pause resume next previous seek queue_add now_playing library_playlists'
DISCLOSURE = 'goes to the agent you connected'


class Mcp:
    def __init__(self, binary, env, out, name):
        self.log = open(os.path.join(out, f'{name}.jsonl'), 'w')
        self.stderr_path = os.path.join(out, f'{name}.stderr')
        self.p = subprocess.Popen([binary, 'mcp'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(self.stderr_path, 'w'), text=True, start_new_session=True)
        self.replies, self.next_id = queue.Queue(), 0
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.p.stdout:
            self.replies.put(json.loads(line))

    def send(self, method, params=None, notify=False, redact=None):
        msg = {'jsonrpc': '2.0', 'method': method}
        if params is not None:
            msg['params'] = params
        if not notify:
            self.next_id += 1
            msg['id'] = self.next_id
        self.p.stdin.write(json.dumps(msg) + '\n')
        self.p.stdin.flush()
        self.log.write(json.dumps({'t': round(time.time(), 2), 'sent': msg}) + '\n')
        if notify:
            return None
        while True:
            reply = self.replies.get(timeout=30)
            if reply.get('id') == msg['id']:
                break
        logged = redact(reply) if redact else reply
        self.log.write(json.dumps({'t': round(time.time(), 2), 'got': logged}) + '\n')
        self.log.flush()
        return reply

    def tool(self, name, args=None, redact=None):
        reply = self.send('tools/call', {'name': name, 'arguments': args or {}}, redact=redact)
        result = reply.get('result') or {}
        text = (result.get('content') or [{}])[0].get('text')
        return bool(result.get('isError')), result.get('structuredContent', text)

    def handshake(self):
        hello = {'protocolVersion': '2025-06-18', 'capabilities': {}, 'clientInfo': {'name': 's12-driver', 'version': '0'}}
        init = self.send('initialize', hello)
        self.send('notifications/initialized', notify=True)
        return init

    def stderr(self):
        return open(self.stderr_path).read()

    def close(self):
        self.p.stdin.close()
        try:
            return self.p.wait(10)
        except subprocess.TimeoutExpired:
            self.p.kill()
            return self.p.wait()
        finally:
            self.log.close()


def network_sockets(pid):
    inodes = set()
    for fd in os.listdir(f'/proc/{pid}/fd'):
        link = os.readlink(f'/proc/{pid}/fd/{fd}')
        if link.startswith('socket:['):
            inodes.add(link[8:-1])
    rows = []
    for table in ('tcp', 'tcp6', 'udp', 'udp6', 'raw', 'raw6'):
        for row in open(f'/proc/{pid}/net/{table}').read().splitlines()[1:]:
            if row.split()[9] in inodes:
                rows.append(f'{table}: {row}')
    return {'sockets': len(inodes), 'network': rows}


def counts_only(reply):
    reply = json.loads(json.dumps(reply))
    sc = (reply.get('result') or {}).get('structuredContent')
    if isinstance(sc, dict) and 'playlists' in sc:
        sc['playlists'] = f"<{len(sc['playlists'])} redacted>"
        reply['result']['content'] = '<redacted: same as structuredContent>'
    return reply


def wait_now(mcp, pred, timeout):
    end, last = time.time() + timeout, None
    while time.time() < end:
        err, now = mcp.tool('now_playing')
        last = now
        if not err and pred(now):
            return now, True
        time.sleep(1)
    return last, False


def drive(a, out, sock, sink, iso, config):
    runtime = os.path.join(out, 'xdg')
    os.makedirs(runtime, mode=0o700)
    uid_dir = f'/run/user/{os.getuid()}'
    out_sink = f'{sink}-missing' if a.mode == 'bad-sink' else sink
    env = dict(os.environ, WAYLAND_DISPLAY=f'{uid_dir}/{sock}', PULSE_SERVER=f'unix:{uid_dir}/pulse/native', PULSE_SINK=out_sink, XDG_RUNTIME_DIR=runtime, TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **iso)
    env.pop('DISPLAY', None)
    cs.assert_isolated(env)
    checks = {}
    v = {'mode': a.mode, 'sink': sink, 'checks': checks}
    probe = Mcp(a.binary, env, out, 'mcp-noapp')
    probe.handshake()
    err, said = probe.tool('now_playing')
    checks['1 no app'] = err and 'not running' in str(said)
    probe.close()
    first = probe.stderr()
    st = os.path.join(out, 'status.json')
    log = open(os.path.join(out, 'app.log'), 'w')
    app = subprocess.Popen([a.binary, '--status', st], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    guard = cs.Guard(app.pid, sink)
    guard.start()
    mcp = None
    try:
        s = cs.wait_for(lambda: (lambda s: s if s and s.get('framesPainted', 0) > 0 and s.get('glRenderer') else None)(cs.status(st)), 30, 'first frame')
        v['glRenderer'] = s['glRenderer']
        v['amd'] = 'AMD' in s['glRenderer']
        mcp = Mcp(a.binary, env, out, 'mcp')
        init = mcp.handshake()
        listed = mcp.send('tools/list')
        names = ' '.join(t['name'] for t in listed['result']['tools'])
        checks['2 handshake'] = init['result']['protocolVersion'] == '2025-06-18' and names == TOOLS
        v['mcpSockets'] = network_sockets(mcp.p.pid)
        err, found = mcp.tool('search', {'term': 'bohemian rhapsody'})
        songs = found.get('songs', []) if isinstance(found, dict) else []
        v['search'] = {'error': err, 'said': None if isinstance(found, dict) else found, 'counts': {k: len(x) for k, x in found.items()} if isinstance(found, dict) else None, 'songInResults': any(x['id'] == SONG for x in songs)}
        checks['3 search'] = not err and len(songs) > 0
        cs.wait_for(lambda: guard.polls > 0, 10, 'a clean guard poll')
        v['routingBeforePlay'] = {'guardPolls': guard.polls, 'sinkExists': cs.sink_exists(sink), 'guardKilled': guard.killed}
        if not cs.sink_exists(sink) or guard.killed:
            raise RuntimeError('null sink missing or guard fired before play; refusing to play')
        sent_at = time.time()
        err, ack = mcp.tool('play', {'id': SONG})
        v['playAck'] = {'error': err, 'reply': ack}
        if a.mode == 'bad-sink':
            now, ok = wait_now(mcp, lambda n: n['state'] == 'Error' and not n['playing'] and n['problem'], 25)
            v['badSink'] = {'now': now, 'guardSeen': sorted(str(t) for t in guard.seen), 'guardKilled': guard.killed}
            checks['bad sink'] = not err and ok and not guard.seen and not guard.killed
            return v
        started = []
        def audible(n):
            if n['playing'] and (n['position'] or 0) > 0 and not started:
                started.append(round(time.time() - n['position'] - sent_at, 2))
            return n['playing'] and n['itemId'] == SONG and n['problem'] is None and (n['position'] or 0) > 35
        now, ok = wait_now(mcp, audible, 90 if not err else 5)
        routed = not guard.killed and guard.seen == {sink}
        v['playing'] = {'now': now, 'audioStartS': started[0] if started else None, 'guardSeen': sorted(str(t) for t in guard.seen), 'guardKilled': guard.killed}
        checks['4 play'] = not err and ok and routed and bool(started) and started[0] <= 3
        if checks['4 play']:
            samples = []
            for _ in range(10):
                samples.append(cs.rollup(app.pid)['Pss'])
                time.sleep(1)
            v['pssMiB'] = round(statistics.median(samples) / 1024, 1)
            v['mcpPssMiB'] = round(cs.rollup(mcp.p.pid)['Pss'] / 1024, 1)
        err, said = mcp.tool('pause')
        now, ok = wait_now(mcp, lambda n: n['state'] == 'Paused' and not n['pending'] and not n['playing'], 5)
        v['paused'] = now
        checks['6 pause'] = not err and ok
        err, said = mcp.tool('queue_add', {'id': QUEUED})
        time.sleep(2)
        _, after = mcp.tool('now_playing')
        v['queueAdd'] = {'error': err, 'said': said if err else None, 'after2s': after}
        checks['5 queue'] = not err and isinstance(after, dict) and after['state'] == 'Paused' and not after['playing']
        err, said = mcp.tool('next')
        time.sleep(3)
        _, after = mcp.tool('now_playing')
        v['nextFromPaused'] = {'error': err, 'after3s': after}
        checks['7 next'] = not err
        err, lib = mcp.tool('library_playlists', redact=counts_only)
        v['library'] = {'error': err, 'total': lib.get('total'), 'returned': lib.get('returned'), 'more': lib.get('more')} if isinstance(lib, dict) else {'error': err, 'said': lib}
        checks['8 library'] = not err and isinstance(lib, dict) and isinstance(lib.get('total'), int)
        v['finalPause'] = mcp.tool('pause')[0]
        v['mcpSocketsEnd'] = network_sockets(mcp.p.pid)
        later = [mcp.stderr()]
        v['mcpExit'] = mcp.close()
        mcp = None
        again = Mcp(a.binary, env, out, 'mcp-again')
        again.handshake()
        time.sleep(0.5)
        later.append(again.stderr())
        again.close()
        no_net = not v['mcpSockets']['network'] and not v['mcpSocketsEnd']['network']
        v['disclosure'] = {'firstRunShown': DISCLOSURE in first, 'recorded': os.path.exists(os.path.join(config, 'mcp-disclosed')), 'laterRunsStderr': later}
        checks['9 mcp process'] = no_net and DISCLOSURE in first and v['disclosure']['recorded'] and later == ['', '']
    finally:
        if mcp:
            mcp.close()
        if app.poll() is None:
            app.terminate()
            try:
                app.wait(15)
            except subprocess.TimeoutExpired:
                app.kill()
                app.wait()
        guard.stop = True
        guard.join(2)
        log.close()
    return v


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', required=True)
    ap.add_argument('--binary', default=os.path.join(ROOT, 'target', 'release', 'bramley'))
    ap.add_argument('--mode', choices=('signed-in', 'signed-out', 'bad-sink'), default='signed-in')
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=False)
    v = {}
    try:
        with cs.session(a.out, 'bramley-s12') as (sock, sink, summary):
            with cs.isolated(f's12-mcp-{a.mode}', a.mode != 'signed-out') as (iso, config):
                v = drive(a, a.out, sock, sink, iso, config)
        v['session'] = summary
    except Exception as e:
        v['exception'] = repr(e)
    s = v.get('session') or {}
    void = [why for why, bad in (('not AMD', not v.get('amd')), ('null sink left', not s.get('sinkGone')), ('default sink changed', s.get('defaultAtEnd') != s.get('defaultAtStart')), ('exception', 'exception' in v)) if bad]
    checks = v.get('checks') or {}
    v['verdict'] = 'VOID' if void else ('PASS' if checks and all(checks.values()) else 'FAIL')
    v['void'] = void
    json.dump(v, open(os.path.join(a.out, 'verdict.json'), 'w'), indent=1)
    print(json.dumps({'verdict': v['verdict'], 'void': void, 'checks': checks}))


if __name__ == '__main__':
    main()
