import argparse, glob, hashlib, json, os, shutil, signal, statistics, subprocess, threading, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
SONG = '6781024437'


def pw_dump():
    return json.loads(subprocess.run(['pw-dump'], capture_output=True, text=True, check=True, timeout=5).stdout)


def props(dump):
    return {o['id']: (o.get('info') or {}).get('props') or {} for o in dump if o.get('type') == 'PipeWire:Interface:Node'}


def sink_exists(name):
    return any(p.get('node.name') == name and p.get('media.class') == 'Audio/Sink' for p in props(pw_dump()).values())


def routing(pid, dump):
    nodes = props(dump)
    ours = [i for i, p in nodes.items() if str(p.get('application.process.id')) == str(pid) and p.get('media.class', '').startswith('Stream/Output')]
    targets = set()
    for o in dump:
        info = o.get('info') or {}
        if o.get('type') == 'PipeWire:Interface:Link' and info.get('output-node-id') in ours:
            targets.add(nodes.get(info.get('input-node-id'), {}).get('node.name'))
    return ours, targets


def default_sink():
    out = subprocess.run(['wpctl', 'inspect', '@DEFAULT_AUDIO_SINK@'], capture_output=True, text=True).stdout
    return next((l.split('=', 1)[1].strip().strip('"') for l in out.splitlines() if 'node.name' in l), None)


class Guard(threading.Thread):
    def __init__(self, pid, allowed):
        super().__init__(daemon=True)
        self.pid, self.allowed = pid, allowed
        self.stop, self.killed, self.first_link, self.seen, self.polls = False, None, None, set(), 0

    def run(self):
        while not self.stop and os.path.exists(f'/proc/{self.pid}'):
            try:
                ours, targets = routing(self.pid, pw_dump())
            except Exception as e:
                self.kill(f'pw-dump failed: {e}')
                return
            self.polls += 1
            if targets and self.first_link is None:
                self.first_link = time.time()
            self.seen |= targets
            bad = sorted(str(t) for t in targets if t != self.allowed)
            if bad:
                self.kill(f'stream linked to {bad}')
                return
            time.sleep(0.25)

    def kill(self, why):
        try:
            os.kill(self.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.killed = {'why': why, 't': time.time()}


def rollup(pid):
    r = {}
    with open(f'/proc/{pid}/smaps_rollup') as f:
        for line in f:
            parts = line.split()
            if len(parts) >= 2 and parts[0].endswith(':') and parts[1].isdigit():
                r[parts[0][:-1]] = int(parts[1])
    return r


def smaps_by_kind(pid):
    kinds, kind = {}, None
    for line in open(f'/proc/{pid}/smaps'):
        parts = line.split()
        if '-' in parts[0] and len(parts) >= 5 and not parts[0].endswith(':'):
            path = parts[5] if len(parts) > 5 else ''
            kind = ('anon' if not path else 'heap' if path == '[heap]' else 'stack' if path.startswith('[stack') else 'dri' if path.startswith('/dev/dri') else 'memfd' if path.startswith('/memfd') else 'binary' if 'bramley' in path else 'shm' if path.startswith('/dev/shm') else 'lib' if '.so' in path else 'other:' + path.rsplit('/', 1)[-1])
        elif parts[0] == 'Pss:':
            kinds[kind] = kinds.get(kind, 0) + int(parts[1])
    return dict(sorted(((k, round(v / 1024, 2)) for k, v in kinds.items()), key=lambda kv: -kv[1]))


def children(pid):
    found = []
    for d in glob.glob('/proc/[0-9]*/stat'):
        try:
            s = open(d).read()
            if int(s[s.rindex(')') + 2:].split()[1]) == pid:
                found.append(int(d.split('/')[2]))
        except (OSError, ValueError):
            pass
    return found


def gpu():
    out = {}
    for key in ('mem_info_vram_used', 'mem_info_gtt_used'):
        vals = [int(open(p).read()) for p in glob.glob(f'/sys/class/drm/card*/device/{key}')]
        out[key] = sum(vals) if vals else None
    return out


def status(path):
    try:
        return json.load(open(path))
    except (OSError, ValueError):
        return None


def wait_for(pred, timeout, what):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(0.2)
    raise RuntimeError(f'timed out waiting for {what}')


def run_one(name, binary, mode, a, sock, sink):
    out = os.path.join(a.out, name)
    st = out + '.status.json'
    for f in glob.glob(out + '.*'):
        os.remove(f)
    env = dict(os.environ, WAYLAND_DISPLAY=sock, PULSE_SINK=sink, TMPDIR=os.path.join(ROOT, 'target', 'tmp'))
    env.pop('DISPLAY', None)
    if mode == 'signedout':
        config = os.path.join(ROOT, 'target', 'tmp', f'signed-out-{os.getpid()}')
        os.makedirs(config, exist_ok=True)
        shutil.copy(os.path.expanduser('~/.config/bramley/developer-token'), config)
        env['BRAMLEY_CONFIG_DIR'] = config
    rc = subprocess.run([binary, 'now'], env=env, capture_output=True).returncode
    if rc != 3:
        raise RuntimeError(f'{name}: slot held (now exit {rc})')
    playing = mode in ('play', 'guardtest', 'shot')
    if playing and not sink_exists(sink):
        raise RuntimeError(f'{name}: null sink {sink} missing before launch; refusing')
    res = {'name': name, 'binary': os.path.basename(binary), 'sha256': hashlib.sha256(open(binary, 'rb').read()).hexdigest()[:16], 'mode': mode, 'defaultBefore': default_sink(), 'gpuBefore': gpu()}
    cmd = [binary, '--status', st]
    if mode == 'shot':
        os.makedirs(os.path.join(ROOT, 'target', 'screens'), exist_ok=True)
        res['screenshot'] = os.path.join(ROOT, 'target', 'screens', f'{name}.png')
        cmd += ['--screenshot', res['screenshot'], '--after', '45']
    log = open(out + '.log', 'w')
    p = subprocess.Popen(cmd, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    guard = Guard(p.pid, 'not-' + sink if mode == 'guardtest' else sink)
    guard.start()
    try:
        wait_for(lambda: (status(st) or {}).get('framesPainted', 0) > 0, 30, 'first frame')
        covers = ('s2b' in name or 'sab' in name or mode == 'shot') and mode != 'signedout'
        if covers:
            s = wait_for(lambda: (lambda s: s if s and (s.get('texturesUploaded') == 50 or s.get('coversFailed')) else None)(status(st)), 60, 'covers')
            res['coversReady'] = {k: s.get(k) for k in ('texturesUploaded', 'decodedBytesHeld', 'coversFailed')}
        if playing:
            if not sink_exists(sink):
                raise RuntimeError(f'{name}: null sink vanished before play; refusing')
            ack = subprocess.run([binary, 'play', SONG], env=env, capture_output=True, text=True)
            res['playAck'] = {'exit': ack.returncode, 'stdout': ack.stdout.strip()[:400], 'stderr': ack.stderr.strip()[:400]}
            res['playAt'] = time.time()
            if ack.returncode != 0:
                raise RuntimeError(f'{name}: play exited {ack.returncode}')
        if mode == 'signedout':
            time.sleep(10)
            res['final'] = status(st)
            res['coversReady'] = {k: res['final'].get(k) for k in ('texturesUploaded', 'coversRequested', 'coversFailed', 'decodedBytesHeld')}
            return res
        if mode == 'guardtest':
            wait_for(lambda: guard.killed, 60, 'guard kill')
            res['guard'] = {'killed': guard.killed['why'], 'linkToKillS': round(guard.killed['t'] - guard.first_link, 2) if guard.first_link else None, 'appGone': p.wait(5) is not None}
            return res
        if mode == 'shot':
            p.wait(120)
            res['screenshotExists'] = os.path.exists(res['screenshot'])
            res['final'] = status(st)
            return res
        if playing:
            wait_for(lambda: (lambda s: s and s['state'] == 'Playing' and s['banner'] is None and (s['position'] or 0) > 15 and s['applied'] >= s['seq'])(status(st)), 90, 'playing past 15 s')
        else:
            time.sleep(12)
        samples = []
        for _ in range(30 if playing else a.samples):
            r = rollup(p.pid)
            s = status(st)
            samples.append({'pss': r['Pss'], 'uss': r.get('Private_Clean', 0) + r.get('Private_Dirty', 0), 'rss': r['Rss'], 'anon': r.get('Anonymous', 0), 'kids': len(children(p.pid)), 'position': s and s['position'], 'state': s and s['state'], 'held': s and s.get('decodedBytesHeld'), 'textures': s and s.get('texturesUploaded')})
            time.sleep(1)
        med = lambda k: round(statistics.median(x[k] for x in samples) / 1024, 1)
        res.update({'pss': med('pss'), 'uss': med('uss'), 'rss': med('rss'), 'anon': med('anon'), 'samples': len(samples), 'maxKids': max(x['kids'] for x in samples), 'positions': [samples[0]['position'], samples[-1]['position']], 'states': sorted(set(str(x['state']) for x in samples)), 'gpuAfter': gpu()})
        res['series'] = samples
        res['smaps'] = smaps_by_kind(p.pid)
        res['final'] = status(st)
        return res
    finally:
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(15)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()
        guard.stop = True
        guard.join(2)
        res['guardSeen'] = sorted(str(t) for t in guard.seen)
        res['guardPolls'] = guard.polls
        res['guardKilled'] = guard.killed and guard.killed['why']
        res['defaultAfter'] = default_sink()
        log.close()
        if mode == 'signedout':
            shutil.rmtree(env['BRAMLEY_CONFIG_DIR'], ignore_errors=True)
        json.dump(res, open(out + '.json', 'w'), indent=1)
        json.dump(verdict(res), open(out + '.verdict.json', 'w'), indent=1)


STATUS_KEYS = ('state', 'position', 'applied', 'seq', 'banner', 'engineStarted', 'framesPainted', 'glRenderer', 'texturesUploaded', 'decodedBytesHeld', 'coversRequested', 'coversFailed')


def verdict(res):
    keep = ('name', 'mode', 'binary', 'sha256', 'pss', 'uss', 'rss', 'anon', 'samples', 'maxKids', 'positions', 'states', 'coversReady', 'smaps', 'gpuBefore', 'gpuAfter', 'guard', 'guardSeen', 'guardPolls', 'guardKilled', 'defaultBefore', 'defaultAfter', 'screenshotExists', 'error')
    v = {k: res[k] for k in keep if k in res}
    v['final'] = {k: (res.get('final') or {}).get(k) for k in STATUS_KEYS}
    if 'playAck' in res:
        v['playAckExit'] = res['playAck']['exit']
    return v


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', required=True)
    ap.add_argument('--samples', type=int, default=20)
    ap.add_argument('runs', nargs='+', help='name=binary:mode')
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    os.makedirs(os.path.join(ROOT, 'target', 'tmp'), exist_ok=True)
    os.environ['XDG_RUNTIME_DIR'] = f'/run/user/{os.getuid()}'
    sock, sink = f'bramley-s2b-{os.getpid()}', f'bramley-s2b-null-{os.getpid()}'
    weston = subprocess.Popen(['weston', '--backend=headless', '--renderer=gl', f'--socket={sock}', '--width=1280', '--height=800', '--idle-time=0', f'--log={a.out}/weston.log'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    node = f'create-node adapter {{ factory.name=support.null-audio-sink node.name={sink} node.description={sink} media.class=Audio/Sink audio.position=[FL FR] priority.session=0 priority.driver=0 object.linger=false }}'
    pw = subprocess.Popen(['bash', '-c', f"{{ echo '{node}'; exec sleep 100000; }} | pw-cli >/dev/null 2>&1"], start_new_session=True)
    summary = {'sink': sink}
    try:
        wait_for(lambda: os.path.exists(f"{os.environ['XDG_RUNTIME_DIR']}/{sock}"), 10, 'weston')
        wait_for(lambda: sink_exists(sink), 10, 'null sink')
        summary['defaultAtStart'] = default_sink()
        if summary['defaultAtStart'] == sink:
            raise RuntimeError('the null sink became the default; refusing')
        for spec in a.runs:
            name, rest = spec.split('=', 1)
            binary, mode = rest.rsplit(':', 1)
            try:
                r = run_one(name, os.path.abspath(binary), mode, a, sock, sink)
            except Exception as e:
                r = {'name': name, 'error': str(e)}
                json.dump(r, open(os.path.join(a.out, name + '.verdict.json'), 'w'))
            keep = ('name', 'error', 'coversRequested', 'pss', 'uss', 'rss', 'anon', 'samples', 'maxKids', 'positions', 'states', 'coversReady', 'guard', 'guardSeen', 'guardKilled', 'screenshotExists', 'defaultBefore', 'defaultAfter')
            line = {k: r[k] for k in keep if k in r}
            line['gl'] = ((r.get('final') or {}).get('glRenderer') or '')[:40]
            line['frames'] = (r.get('final') or {}).get('framesPainted')
            print(json.dumps(line), flush=True)
    finally:
        os.killpg(pw.pid, signal.SIGTERM)
        os.killpg(weston.pid, signal.SIGTERM)
        time.sleep(1.5)
        summary['sinkGone'] = not sink_exists(sink)
        summary['defaultAtEnd'] = default_sink()
        print(json.dumps(summary), flush=True)
        json.dump(summary, open(os.path.join(a.out, 'session.json'), 'w'))


if __name__ == '__main__':
    main()
