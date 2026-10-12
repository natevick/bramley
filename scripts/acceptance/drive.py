import argparse, contextlib, glob, hashlib, http.server, importlib.util, json, os, queue, shutil, signal, subprocess, sys, threading, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, os.path.join(ROOT, path))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cs, gate = load('covers_session', 'scripts/app/covers-session.py'), load('gate', 'scripts/acceptance/gate.py')
s6, s9 = load('s6_shots', 'scripts/app/s6-shots.py'), load('s9_session', 'scripts/app/s9-session.py')
SONG, SECOND = '6781024437', '6781079518'
ALBUM = ['6781024028', '6781024424', '6781024426', '6781024428', '6781024429', '6781024431', '6781024433', '6781024434', '6781024435',
         '6781024436', '6781024437', '6781024440', '6781024441', '6781024442', '6781024443', '6781024444', '6781024445', '6781024448']
F8_ORDER, TERM, BAD_ID = [ALBUM[9], ALBUM[0], ALBUM[6], ALBUM[3]], 'bohemian rhapsody', '1'
SOAK_S, SOAK_EVERY_S, SOAK_TRACK_S, F6_WINDOW_S, RECOVER_S, CUT_BEFORE_END_S = 1800, 10, 150, 5, 90, 3
NAMES = {'library': set(), 'catalog': set()}
SNAP_KEYS = ('pid', 'state', 'position', 'duration', 'itemId', 'banner', 'engineStarted', 'texturesUploaded', 'coversFailed', 'framesPainted',
             'glRenderer', 'applied', 'seq', 'engine', 'sentMs', 'decodedBytesKept')
STANDING_GPU_CLIENTS, GPU_LIBS = ('gnome-shell', 'Xvfb', 'grok-bot'), ('libgallium', 'radeonsi', 'libLLVM')
TICK, RUNTIME, CPU_EVERY_S = os.sysconf('SC_CLK_TCK'), os.path.join(ROOT, 'target', 'x'), 2


def note_names(found, kind='catalog'):
    if isinstance(found, dict):
        NAMES[kind].update(v for k, v in found.items() if k in ('name', 'title', 'artist', 'album') and isinstance(v, str) and len(v) >= 4)
        [note_names(v, kind) for v in found.values()]
    elif isinstance(found, list):
        [note_names(v, kind) for v in found]


def leak_grep(out):
    tokens = [open(os.path.join(cs.REAL_CONFIG, f)).read().strip() for f in ('developer-token', 'music-user-token') if os.path.exists(os.path.join(cs.REAL_CONFIG, f))]
    fragments = [t[i:i + 24] for t in tokens for i in (0, len(t) // 2, len(t) - 24)]
    private, every = sorted(NAMES['library']) + fragments, sorted(NAMES['library'] | NAMES['catalog']) + fragments
    git = lambda *args: subprocess.run(['git', *args], cwd=ROOT, capture_output=True, text=True).stdout
    repo = '\n'.join([line for line in git('diff', 'main..HEAD').splitlines() if line.startswith('+')] + [git('log', 'main..HEAD', '--format=%B')]).lower()
    files = []
    for d, dirs, names in os.walk(out):
        dirs[:] = [x for x in dirs if x not in ('bin', 'empty')]
        files += [open(os.path.join(d, f), errors='replace').read() for f in names if not f.endswith('.png')]
    run = '\n'.join(files).lower()
    count = lambda body, needles: sum(body.count(n.lower()) for n in needles)
    hits = {'repo': count(repo, private), 'run': count(run, every)}
    planted = bool(NAMES['library']) and count(run + every[0].lower(), every) > hits['run'] and count(repo + private[0].lower(), private) > hits['repo']
    gate.save(out, 'leakgrep.json', {'libraryNames': len(NAMES['library']), 'catalogNames': len(NAMES['catalog']), 'tokenFragments': len(fragments),
                                     'runFiles': len(files), 'hits': hits, 'plantedCaught': planted})


def guard_fired(out):
    return sorted(f for f in os.listdir(out) if f.endswith('.guard.json') and (gate.load(out, f) or {}).get('killed'))


def preflight():
    found = []
    for d in glob.glob('/proc/[0-9]*'):
        try:
            comm = open(f'{d}/comm').read().strip()
            libs = sorted({line.split()[-1].rsplit('/', 1)[-1] for line in open(f'{d}/maps') if any(lib in line for lib in GPU_LIBS)})
        except OSError:
            libs = []
        if int(d[6:]) != os.getpid() and (comm in ('weston', 'bramley') or (libs and comm not in STANDING_GPU_CLIENTS)):
            found.append(f'{d[6:]} {comm} {libs}')
    return found


def mem(pid):
    rolls = [cs.rollup(p) for p in [pid] + cs.children(pid)]
    total = lambda *keys: round(sum(r.get(k, 0) for r in rolls for k in keys) / 1024, 2)
    return {'pss': total('Pss'), 'uss': total('Private_Clean', 'Private_Dirty'), 'rss': total('Rss'), 'procs': len(rolls)}


def ticks(pid):
    total = 0
    for p in [pid] + cs.children(pid):
        fields = open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()
        total += int(fields[11]) + int(fields[12])
    return total


class App:
    def __init__(self, out, name, binary, env, sink, args=(), wrap=(), status=True):
        self.out, self.name, self.binary, self.env, self.sink = out, name, binary, env, sink
        self.st = os.path.join(out, f'{name}.status.json') if status else None
        cs.assert_isolated(env)
        if subprocess.run([binary, 'now'], env=env, capture_output=True).returncode != 3:
            raise RuntimeError(f'{name}: the instance slot is held before launch; refusing')
        self.log = open(os.path.join(out, f'{name}.log'), 'w')
        self.t_exec = time.time()
        self.p = subprocess.Popen([*wrap, binary, *(['--status', self.st] if status else []), *args], env=env, stdout=self.log, stderr=subprocess.STDOUT, start_new_session=True)
        self.pid = self.p.pid if not wrap else cs.wait_for(lambda: self.snap().get('pid'), 30, f'{name} pid')
        self.guard = cs.Guard(self.pid, sink)
        self.guard.start()

    def snap(self):
        return (cs.status(self.st) if self.st else self.cli('now')[1]) or {}

    def cli(self, *args):
        t = time.time()
        r = subprocess.run([self.binary, *args], env=self.env, capture_output=True, text=True, timeout=30)
        try:
            reply = json.loads(r.stdout.splitlines()[0])
        except (IndexError, ValueError):
            reply = {}
        return r.returncode, reply, round((time.time() - t) * 1000)

    def until(self, pred, timeout, step=0.01):
        end, s, step = time.time() + timeout, {}, step if self.st else max(step, 0.5)
        while time.time() < end:
            s = self.snap()
            if pred(s):
                return s, time.time()
            time.sleep(step)
        return s, None

    def safe_to_play(self):
        cs.wait_for(lambda: self.guard.polls > 0, 10, 'a clean guard poll')
        if not cs.sink_exists(self.sink) or cs.default_sink() in (None, self.sink) or self.guard.killed:
            raise RuntimeError(f'{self.name}: audio not safe before play; refusing')

    def renderers(self):
        return [self.snap().get('glRenderer')]

    def close(self):
        for sig, wait in ((signal.SIGTERM, 15), (signal.SIGKILL, 5)):
            with contextlib.suppress(ProcessLookupError):
                os.killpg(self.p.pid, sig)
            with contextlib.suppress(subprocess.TimeoutExpired):
                self.p.wait(wait)
                break
        self.guard.stop = True
        self.guard.join(2)
        self.log.close()
        gate.save(self.out, f'{self.name}.guard.json', {'pid': self.pid, 'polls': self.guard.polls, 'killed': self.guard.killed and self.guard.killed['why'],
                                                        'seen': sorted(map(str, self.guard.seen)), 'log': self.guard.log})


@contextlib.contextmanager
def app(*a, **k):
    x = App(*a, **k)
    try:
        yield x
    finally:
        x.close()


def evidence(out, name, ev, session):
    ev.update(session=session.name, t1=time.time(), renderers=session.renderers())
    gate.save(out, name, ev)


def sample(a, n, every=1.0, extra=None):
    rows = []
    for _ in range(n):
        t = time.time()
        s = a.cli('now')[1]
        rows.append(dict(mem(a.pid), t=round(t, 2), **{k: s.get(k) for k in SNAP_KEYS}, **(extra() if extra else {})))
        time.sleep(max(0.0, every - (time.time() - t)))
    return rows


def cpu(a, state, name, pre):
    ev = {'session': a.name, 't0': time.time(), 'before': a.snap().get('state')}
    rows, last = [], ticks(a.pid)
    for _ in range(gate.CPU_SAMPLES):
        time.sleep(CPU_EVERY_S)
        now = ticks(a.pid)
        rows.append(round(100 * (now - last) / TICK / CPU_EVERY_S, 2))
        last = now
    ev.update(cpu=rows, after=a.snap().get('state'), started=state == 'Playing')
    evidence(a.out, f'{pre}{name}.json', ev, a)


def applied(reply, effect, same_engine=True):
    return lambda s: (s.get('engine') == reply.get('engine') or not same_engine) and (s.get('applied') or 0) >= (reply.get('seq') or 1 << 62) and effect(s)


def play_song(a, song, reps, ev):
    ev.update(song=song, reps=[], started=False)
    for _ in range(reps):
        t = time.time()
        code, reply, _ = a.cli('play', song)
        fresh = lambda s: s.get('itemId') == song and s.get('state') == 'Playing' and 0 < (s.get('position') or 0) < 3
        s, seen = a.until(applied(reply, fresh, same_engine=False), 15)
        ev['reps'].append({'exit': code, 'audioMs': seen and round((seen - t) * 1000)})
        ev['started'] |= seen is not None
        if seen:
            a.until(lambda s: (s.get('position') or 0) > 3, 10)
    s, _ = a.until(lambda s: s.get('itemId') == song and (s.get('position') or 0) > gate.F4_PLAYED_S + 2, 60, 0.5) if ev['started'] else (a.snap(), None)
    ev['final'] = {k: s.get(k) for k in ('itemId', 'state', 'position', 'duration', 'sentMs', 'banner')}
    note_names(a.cli('now')[1])


def f4(a, pre):
    a.safe_to_play()
    ev = {'t0': time.time()}
    play_song(a, SONG, gate.REPS, ev)
    evidence(a.out, f'{pre}f4.json', ev, a)


def f5(a, pre):
    ev, effects = {'t0': time.time(), 'rounds': [], 'started': False}, {
        'pause': ('pause',), 'seek': ('seek', '120'), 'resume': ('play',), 'next': ('next',), 'prev': ('prev',)}
    shows = {'pause': lambda s: s.get('state') == 'Paused', 'seek': lambda s: 119.5 <= (s.get('position') or 0) <= 122,
             'resume': lambda s: s.get('state') == 'Playing', 'next': lambda s: s.get('itemId') == SECOND, 'prev': lambda s: s.get('itemId') == SONG}
    for _ in range(gate.REPS):
        a.cli('play', SONG)
        s, ok = a.until(lambda s: s.get('itemId') == SONG and s.get('state') == 'Playing' and (s.get('position') or 0) > 3, 20)
        if not ok:
            break
        ev['started'] = True
        a.cli('queue', 'add', SECOND)
        a.until(lambda s: (s.get('queueLength') or 0) >= 2, 5)
        row = {}
        for cmd in gate.F5_COMMANDS:
            t = time.time()
            code, reply, _ = a.cli(*effects[cmd])
            _, acked = a.until(applied(reply, lambda s: True), 3, 0.005)
            s, seen = a.until(applied(reply, shows[cmd]), 3, 0.005)
            row[cmd] = {'exit': code, 'matched': seen is not None, 'ms': seen and round((seen - t) * 1000), 'appliedMs': acked and round((acked - t) * 1000)}
            a.until(lambda s: s.get('state') == 'Playing' and (s.get('position') or 0) > 1, 10) if cmd in ('resume', 'next') else None
        ev['rounds'].append(row)
    evidence(a.out, f'{pre}f5.json', ev, a)


def transition(a, ids, at, start, cut=False):
    s, ok = a.until(lambda s: s.get('itemId') == ids[at] and s.get('state') == 'Playing' and (s.get('duration') or 0) > 30 and (s.get('position') or 0) > start + 1, 40)
    if not ok:
        return {'reached': False, 'pass': False}
    duration, target = s['duration'], round(s['duration'] - 20, 1)
    reply = a.cli('seek', str(target))[1]
    base, land, flip, samples, end, cut_sent = None, None, None, [], time.time() + 45, False
    while time.time() < end and (flip is None or time.time() < flip[0] + F6_WINDOW_S):
        s, now = a.snap(), time.time()
        pos = s.get('position') or 0
        samples.append((now, pos))
        if base is None and applied(reply, lambda s: True)(s):
            base, land = {k: s.get(k) or 0 for k in ('drainsAcked', 'drainTimeouts', 'sentMs', 'skippedMs')}, pos
        if flip is None and base and s.get('itemId') == ids[at + 1]:
            flip = (now, {k: (s.get(k) or 0) - base[k] for k in base}, pos)
        if flip and base:
            after = {k: (s.get(k) or 0) - base[k] for k in base}
        if cut and base and not flip and not cut_sent and s.get('itemId') == ids[at] and pos - start >= duration - CUT_BEFORE_END_S:
            a.cli('next')
            cut_sent = True
        time.sleep(0.02)
    if flip is None or land is None:
        return {'advanced': False, 'pass': False}
    window = [(t, p) for t, p in samples if abs(t - flip[0]) <= F6_WINDOW_S]
    stall, moved = 0.0, window[0][0]
    for (t0, p0), (t1, p1) in zip(window, window[1:]):
        moved = t1 if p1 > p0 else moved
        stall = max(stall, t1 - moved)
    d = flip[1]
    res = {'acked': d['drainsAcked'], 'timeouts': d['drainTimeouts'], 'sentS': after['sentMs'] / 1000, 'skippedS': after['skippedMs'] / 1000,
           'stallS': round(stall, 2), 'landedOff': round(land - start - target, 2), 'nextStart': flip[2], 'cut': cut_sent}
    res['drained'] = res['acked'] >= 1 and res['timeouts'] == 0
    res['gapless'] = res['sentS'] >= (duration - target) - 3 and res['skippedS'] <= 0.1 and stall <= 1
    res['pass'] = abs(res['landedOff']) <= 1 and (res['drained'] or res['gapless'])
    return res


def queue_up(a, ids, ev):
    ev['exits'] = [a.cli('play', ids[0])[0]]
    ev['started'] = a.until(lambda s: s.get('itemId') == ids[0] and s.get('state') == 'Playing' and (s.get('position') or 0) > 1, 30)[1] is not None
    ev['exits'] += [a.cli('queue', 'add', i)[0] for i in ids[1:]]
    a.until(lambda s: (s.get('queueLength') or 0) >= len(ids), 10)
    return ev['started']


def f6(a, pre, cut=False):
    ids, ev = ALBUM[2:6], {'t0': time.time()}
    queue_up(a, ids, ev)
    ev['transitions'], start = [], 0.0
    for i in range(3):
        ev['transitions'].append(transition(a, ids, i, start, cut and i == 2))
        start = ev['transitions'][-1].get('nextStart', start)
    evidence(a.out, f'{pre}f6.json', ev, a)


def f8(a, pre):
    ev = {'t0': time.time(), 'expected': F8_ORDER, 'seen': []}
    queue_up(a, F8_ORDER, ev)
    end = time.time() + 120
    while time.time() < end and len(ev['seen']) < len(F8_ORDER):
        s = a.snap()
        item = s.get('itemId')
        if item and s.get('state') == 'Playing' and item not in ev['seen'][-1:]:
            ev['seen'].append(item)
            if len(ev['seen']) < len(F8_ORDER):
                a.until(lambda s: (s.get('duration') or 0) > 0 and (s.get('position') or 0) > 1, 15)
                a.cli('seek', str(max(0.0, (a.snap().get('duration') or 30) - 5)))
        time.sleep(0.1)
    evidence(a.out, f'{pre}f8.json', ev, a)


def r3(a, pre):
    a.safe_to_play()
    ev = {'pid': a.pid, 't0': time.time()}
    queue_up(a, ALBUM, ev)
    seeks, starts, sought, start = [0], {}, set(), time.time()

    def pace():
        s = a.snap()
        item, pos, dur = s.get('itemId'), s.get('position') or 0, s.get('duration') or 0
        if item and s.get('state') == 'Playing':
            starts.setdefault(item, pos)
        if item in starts and item not in sought and dur > SOAK_TRACK_S + 15 and pos - starts[item] >= SOAK_TRACK_S:
            sought.add(item)
            a.cli('seek', str(round(dur - 10, 1)))
            seeks[0] += 1
        return {'m': round((time.time() - start) / 60, 3), 'seeks': seeks[0]}

    rows = []
    while time.time() - start < SOAK_S + SOAK_EVERY_S:
        rows += sample(a, 1, 0, pace)
        for _ in range(SOAK_EVERY_S):
            time.sleep(1)
            pace()
    ev['samples'] = rows
    evidence(a.out, f'{pre}r3.json', ev, a)


def memory(a, name, extra):
    r1 = dict(extra, binary=os.path.basename(a.binary), pid=a.pid, gpuBefore=cs.gpu(), samples=[], started=False)
    try:
        a.safe_to_play()
        r1['prePlay'] = {'sinkExists': cs.sink_exists(a.sink), 'default': cs.default_sink(), 'guardPolls': a.guard.polls}
        ev = {'t0': time.time()}
        play_song(a, SONG, 1, ev)
        r1['started'] = ev['started']
        if ev['started']:
            r1['samples'] = sample(a, gate.R1_SAMPLES)
        r1['gpuAfter'] = cs.gpu()
    finally:
        gate.save(a.out, f'{name}.r1.json', r1)


def cpu_session(out, binary, env, sink):
    with app(out, 'cpu', binary, env, sink, status=False) as a:
        a.until(lambda s: (s.get('texturesUploaded') or 0) >= gate.COVERS, 60)
        a.safe_to_play()
        a.cli('play', SONG)
        a.until(lambda s: s.get('itemId') == SONG and s.get('state') == 'Playing' and (s.get('position') or 0) > 10, 40)
        cpu(a, 'Playing', 'r4', '')
        a.cli('pause')
        time.sleep(3)
        cpu(a, 'Paused', 'r5', '')


def main_session(out, binary, env, sink, pre, steps, extra=None):
    name = f'{pre}shipped' if not extra else 'ballast'
    with app(out, name, binary, env, sink) as a:
        a.until(lambda s: s.get('framesPainted', 0) > 0 and ((s.get('texturesUploaded') or 0) >= gate.COVERS or s.get('coversFailed')), 60)
        time.sleep(5)
        if 'r2' in steps:
            gate.save(out, f'{name}.r2.json', {'binary': os.path.basename(binary), 'pid': a.pid, 'samples': sample(a, gate.R2_SAMPLES)})
        for step in steps:
            if a.guard.killed:
                raise RuntimeError(f'{name}: the guard fired: {a.guard.killed}')
            {'f4': f4, 'f5': f5, 'f6': f6, 'f8': f8, 'r3': r3, 'r2': lambda a, p: None, 'r1': lambda a, p: memory(a, name, extra or {}),
             'cut': lambda a, p: f6(a, p, cut=True),
             }[step](a, pre)


def f1(out, binary, env, sink, pre=''):
    ev = {'starts': []}
    for i in range(gate.REPS):
        with app(out, f'{pre}f1-{i}', binary, env, sink) as a:
            s, seen = a.until(lambda s: s.get('framesPainted', 0) > 0, 10, 0.002)
            listed, _ = a.until(lambda s: ((s.get('library') or {}).get('playlists') or {}).get('loaded'), 20)
            ev['starts'].append({'ms': seen and round((seen - a.t_exec) * 1000), 'route': listed.get('route'), 'playlists': ((listed.get('library') or {}).get('playlists') or {}).get('loaded')})
            ev.setdefault('renderers', []).append(s.get('glRenderer'))
    gate.save(out, f'{pre}f1.json', ev)


def f2(out, binary, env, sink, pre=''):
    with app(out, f'{pre}signedout', binary, env, sink) as a:
        ev = {'t0': time.time()}
        s, _ = a.until(lambda s: s.get('banner') == 'SignInNeeded', 20)
        ev['playExit'] = a.cli('play', SONG)[0]
        _, played = a.until(lambda s: s.get('state') == 'Playing' or s.get('engineStarted'), 10)
        ev.update(banner=a.snap().get('banner'), played=played is not None, streams=sorted(map(str, a.guard.seen)))
        evidence(out, f'{pre}f2.json', ev, a)


class Mcp:
    def __init__(self, binary, env, path):
        self.p = subprocess.Popen([binary, 'mcp'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(path, 'w'), text=True, start_new_session=True)
        self.replies, self.next_id = queue.Queue(), 0
        threading.Thread(target=lambda: [self.replies.put(json.loads(line)) for line in self.p.stdout], daemon=True).start()

    def send(self, method, params=None, notify=False):
        msg = dict({'jsonrpc': '2.0', 'method': method}, **({'params': params} if params is not None else {}))
        if not notify:
            self.next_id += 1
            msg['id'] = self.next_id
        self.p.stdin.write(json.dumps(msg) + '\n')
        self.p.stdin.flush()
        while not notify:
            reply = self.replies.get(timeout=30)
            if reply.get('id') == msg['id']:
                return reply

    def tool(self, name, args=None):
        result = self.send('tools/call', {'name': name, 'arguments': args or {}}).get('result') or {}
        return bool(result.get('isError')), result.get('structuredContent')

    def close(self):
        self.p.stdin.close()
        try:
            self.p.wait(10)
        except subprocess.TimeoutExpired:
            self.p.kill()


def f9(a, pre, mcp):
    ev = {'t0': time.time(), 'song': SONG, 'reps': [], 'started': False}
    ev['tools'] = sorted(t['name'] for t in mcp.send('tools/list')['result']['tools'])
    err, found = mcp.tool('search', {'term': TERM})
    note_names(found)
    ev['searchHasSong'] = not err and any(x.get('id') == SONG for x in (found or {}).get('songs', []))
    a.safe_to_play()
    for _ in range(gate.REPS):
        t = time.time()
        err, said = mcp.tool('play', {'id': SONG})
        s, seen = a.until(lambda s: s.get('itemId') == SONG and s.get('state') == 'Playing' and 0 < (s.get('position') or 0) < 3, 15)
        ev['reps'].append({'error': err, 'audioMs': seen and round((seen - t) * 1000)})
        ev['started'] |= seen is not None
        a.until(lambda s: (s.get('position') or 0) > 3, 10)
    s, _ = a.until(lambda s: s.get('itemId') == SONG and (s.get('position') or 0) > gate.F4_PLAYED_S + 2, 60, 0.5) if ev['started'] else (a.snap(), None)
    ev['final'] = {k: s.get(k) for k in ('itemId', 'state', 'position', 'duration', 'sentMs', 'banner')}
    mcp.tool('pause')
    evidence(a.out, f'{pre}f9.json', ev, a)


def f3(a, pre):
    ev = {'t0': time.time(), 'reps': []}
    for _ in range(gate.REPS):
        mark = (a.snap().get('search') or {}).get('seq') or 0
        t = time.time()
        code, reply, wall = a.cli('search', TERM)
        songs = (((reply.get('results') or {}).get('songs') or {}).get('items') or [])
        found = lambda x: x.get('state') == 'results' and x.get('results', 0) > 0 and (x.get('seq') or 0) > mark
        _, seen = a.until(lambda s: s.get('route') == 'Search' and found(s.get('search') or {}), 5, 0.005)
        ev['reps'].append({'exit': code, 'wallMs': wall, 'songs': len(songs), 'uiMs': seen and round((seen - t) * 1000)})
        time.sleep(1)
    evidence(a.out, f'{pre}f3.json', ev, a)


def largest_playlist(config):
    tokens = [open(os.path.join(config, f)).read().strip() for f in ('developer-token', 'music-user-token')]
    sizes = {p['id']: s9.api(f"/v1/me/library/playlists/{p['id']}/tracks?limit=1", tokens).get('meta', {}).get('total', 0) for p in s9.every('/v1/me/library/playlists', tokens)[0]}
    best = max(sizes, key=sizes.get) if sizes else None
    return best, sizes.get(best)


def browse_session(out, binary, env, sink, config):
    playlist, tracks = largest_playlist(config)
    with app(out, 'browse', binary, env, sink) as a:
        a.until(lambda s: s.get('framesPainted', 0) > 0 and (s.get('library') or {}).get('playlists', {}).get('pages'), 30)
        f3(a, '')
        mcp = Mcp(binary, env, os.path.join(out, 'mcp.stderr'))
        try:
            mcp.send('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {}, 'clientInfo': {'name': 'acceptance', 'version': '0'}})
            mcp.send('notifications/initialized', notify=True)
            f9(a, '', mcp)
            err, listed = mcp.tool('library_playlists')
            note_names(listed, 'library')
            gate.save(out, 'f7.mcp.json', {'error': err, 'total': (listed or {}).get('total'), 'largestTracks': tracks})
        finally:
            mcp.close()
        ev = {'t0': time.time(), 'pidBefore': a.snap().get('pid')}
        code, reply, _ = a.cli('play', BAD_ID)
        s, _ = a.until(lambda s: s.get('banner') or s.get('state') == 'Error', 20)
        time.sleep(3)
        ev.update(exit=code, refused=reply.get('refused') or reply.get('error'), banner=a.snap().get('banner'), alive=a.p.poll() is None and a.cli('now')[1].get('pid') == ev['pidBefore'])
        ev['error'] = bool(ev['banner'] or ev['refused'])
        evidence(out, 'f10-badid.json', ev, a)
    return playlist


def offline_session(out, binary, env, sink):
    resolv = os.path.join(out, 'netns-resolv.conf')
    open(resolv, 'w').write('nameserver 10.0.2.3\n')
    wrap = ('bwrap', '--dev-bind', '/', '/', '--ro-bind', resolv, '/etc/resolv.conf', '--unshare-net', '--die-with-parent', '--')
    slirp = None
    with app(out, 'offline', binary, env, sink, wrap=wrap) as a:
        ev = {'t0': time.time()}
        try:
            a.until(lambda s: s.get('framesPainted', 0) > 0, 30)
            time.sleep(10)
            code, _, _ = a.cli('search', TERM)
            ui, _ = a.until(lambda s: (s.get('search') or {}).get('state') == 'error', 10)
            ev.update(downError=code != 0, downUi=(ui.get('search') or {}).get('state'), coversWhileDown=a.snap().get('texturesUploaded'), alive=a.p.poll() is None)
            slirp = subprocess.Popen(['slirp4netns', '--configure', '--mtu=65520', '--disable-host-loopback', str(a.pid), 'tap0'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            up = time.time()
            _, back = a.until(lambda s: (s.get('texturesUploaded') or 0) >= gate.COVERS, RECOVER_S, 0.5)
            ev['coversRecoveredS'] = back and round(back - up, 1)
            code, _, _ = a.cli('search', TERM)
            ui, _ = a.until(lambda s: (s.get('search') or {}).get('state') == 'results', 10)
            ev.update(searchAfter=code == 0, uiAfter=(ui.get('search') or {}).get('state'), aliveAtEnd=a.p.poll() is None)
        finally:
            if slirp:
                slirp.terminate()
                slirp.wait(5)
            evidence(out, 'f10-net.json', ev, a)


def dev_views(out, dev, env, sink, playlist, run):
    shots = os.path.join(ROOT, 'target', 'screens', 'PRIVATE-real-art', 'acceptance', run)
    os.makedirs(shots, exist_ok=True)
    open(os.path.join(os.path.dirname(shots), s6.MARK['real']), 'w').close()
    views = {'home-playing': ['--after', '45'], 'search': ['--route', 'search', '--search', TERM, '--after', '10'],
             'playlist': ['--route', f'playlist:{playlist}', '--scroll-end', '6', '--after', '50']}
    made = {}
    for view, args in views.items():
        if view == 'playlist' and not playlist:
            continue
        png = os.path.join(shots, f'{view}.png')
        with app(out, f'dev-{view}', dev, env, sink, args=['--screenshot', png, *args]) as a:
            if view == 'home-playing':
                a.until(lambda s: (s.get('texturesUploaded') or 0) >= gate.COVERS, 30)
                a.safe_to_play()
                a.cli('play', SONG)
            a.p.wait(120)
            s = a.snap()
        made[view] = os.path.exists(png)
        if view == 'playlist':
            lib, mcp = s.get('library') or {}, gate.load(out, 'f7.mcp.json') or {}
            tracks = lib.get('tracks') or {}
            gate.save(out, 'f7.json', {'driver': 'MCP list on shipped; open, scroll and covers on the dev build (deviation pending approval)', 'mcpTotal': mcp.get('total'),
                                       'largestTracks': mcp.get('largestTracks'), 'endReached': tracks.get('loaded') == tracks.get('total') == lib.get('shownTo') and (tracks.get('pages') or 0) > 1,
                                       'artTextures': lib.get('artTextures'), 'renderers': [s.get('glRenderer')]})
    gate.save(out, 'screenshots.json', {'dir': os.path.relpath(shots, ROOT), 'made': made})


def reference(out, sock, sink, name):
    with cs.isolated(f'{os.path.basename(out)}-{name}', True) as (iso, _):
        env = env_for(sock, sink, **iso)
        ev = {'binary': os.path.basename(gate.REFERENCE['binary']), 'sha256': sha(gate.REFERENCE['binary'])}
        try:
            with app(out, name, gate.REFERENCE['binary'], env, sink) as a:
                ev['pid'] = a.pid
                cs.wait_for(lambda: a.cli('now')[1].get('framesPainted', 0) > 0, 30, 'reference first frame')
                time.sleep(12)
                ev['samples'] = sample(a, gate.REF_SAMPLES)
        finally:
            gate.save(out, f'{name}.json', ev)


def sha(path):
    return hashlib.sha256(open(path, 'rb').read()).hexdigest()[:16]


def env_for(sock, sink, llvmpipe=False, **extra):
    uid_dir, runtime = f'/run/user/{os.getuid()}', os.path.join(RUNTIME, hashlib.sha256(extra['BRAMLEY_CONFIG_DIR'].encode()).hexdigest()[:8])
    os.makedirs(runtime, mode=0o700, exist_ok=True)
    env = dict(os.environ, WAYLAND_DISPLAY=f'{uid_dir}/{sock}', PULSE_SERVER=f'unix:{uid_dir}/pulse/native', XDG_RUNTIME_DIR=runtime, PULSE_SINK=sink,
               TMPDIR=os.path.join(ROOT, 'target', 'tmp'), **extra)
    env.pop('DISPLAY', None)
    if llvmpipe:
        env['LIBGL_ALWAYS_SOFTWARE'] = '1'
    return env


def real_state():
    token = os.path.join(cs.REAL_CONFIG, 'music-user-token')
    st = os.stat(token) if os.path.exists(token) else None
    cache = {f: [s.st_mtime_ns, s.st_size] for f in sorted(os.listdir(cs.REAL_CACHE)) for s in [os.stat(os.path.join(cs.REAL_CACHE, f))]} if os.path.isdir(cs.REAL_CACHE) else None
    return {'tokenMtimeNs': st and st.st_mtime_ns, 'tokenSize': st and st.st_size, 'keyringItems': cs.keyring_items('bramley'), 'cache': cache}


@contextlib.contextmanager
def fixture_token(config):
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), s6.Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        yield ['--developer-token-url', f'http://127.0.0.1:{server.server_address[1]}/developer-token.json']
    finally:
        server.shutdown()


def phases(out, mode, bins, sock, sink):
    run, b = os.path.basename(out), lambda n: os.path.join(bins, n)
    shipped, sabotage, dev = b('bramley'), b('bramley-sabotage'), b('bramley-dev')
    iso = lambda tag, signed_in=True: cs.isolated(f'{run}-{tag}', signed_in)

    def signed_in(tag, go, llvmpipe=mode == 'llvmpipe', **extra):
        with iso(tag) as (env, config):
            return go(env_for(sock, sink, llvmpipe, **env, **extra), config)

    def signed_out(tag, go):
        with iso(tag, False) as (env, _):
            return go(env_for(sock, sink, **env))

    found = {}
    yield 'reference', lambda: reference(out, sock, sink, 'reference')
    if mode == 'llvmpipe':
        yield 'f1', lambda: signed_in('f1', lambda e, c: f1(out, shipped, e, sink))
        yield 'shipped', lambda: signed_in('shipped', lambda e, c: main_session(out, shipped, e, sink, '', ('r2', 'f4', 'r1')))
    elif mode == 'controls':
        yield 'nouser', lambda: signed_out('nouser', lambda e: (f2(out, shipped, e, sink, 'nouser.'), main_session(out, shipped, e, sink, 'nouser.', ('f4', 'r1', 'f5', 'f6')),
                                                                 browse_nouser(out, shipped, e, sink)))
        yield 'badtoken', lambda: signed_in('badtoken', lambda e, c: bad_token(out, dev, e, sink, c))
        yield 'cut', lambda: signed_in('cut', lambda e, c: main_session(out, shipped, e, sink, 'cut.', ('cut',)))
        yield 'nostream', lambda: signed_in('nostream', lambda e, c: main_session(out, shipped, dict(e, PULSE_SINK=sink + '-missing'), sink, 'nostream.', ('f4',)))
        yield 'leak', lambda: signed_in('leak', lambda e, c: main_session(out, sabotage, e, sink, 'leak.', ('r3',)), BRAMLEY_GATE_LEAK_MIB='3')
    else:
        yield 'f1', lambda: signed_in('f1', lambda e, c: f1(out, shipped, e, sink))
        yield 'f2', lambda: signed_out('f2', lambda e: f2(out, shipped, e, sink))
        yield 'shipped', lambda: signed_in('shipped', lambda e, c: main_session(out, shipped, e, sink, '', ('r2', 'r1', 'f4', 'f5', 'f6', 'f8', 'r3')))
        yield 'cpu', lambda: signed_in('cpu', lambda e, c: cpu_session(out, shipped, e, sink))
        yield 'ballast', lambda: ballast_run(out, sabotage, sink, signed_in)
        yield 'browse', lambda: signed_in('browse', lambda e, c: found.update(playlist=browse_session(out, shipped, e, sink, c)))
        yield 'offline', lambda: signed_in('offline', lambda e, c: offline_session(out, shipped, e, sink))
        yield 'dev', lambda: signed_in('dev', lambda e, c: dev_views(out, dev, e, sink, found.get('playlist'), run))
    yield 'reference-after', lambda: reference(out, sock, sink, 'reference-after')


def ballast_run(out, sabotage, sink, signed_in):
    mib = gate.ballast_size(out)
    if mib is None:
        raise RuntimeError('no shipped R1 median to size the ballast from')
    signed_in('ballast', lambda e, c: main_session(out, sabotage, e, sink, '', ('r2', 'r1'), {'ballastMiB': mib}), BRAMLEY_GATE_BALLAST_MIB=str(mib))


def browse_nouser(out, binary, env, sink):
    with app(out, 'nouser.browse', binary, env, sink) as a:
        a.until(lambda s: s.get('framesPainted', 0) > 0, 30)
        mcp = Mcp(binary, env, os.path.join(out, 'nouser.mcp.stderr'))
        try:
            mcp.send('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {}, 'clientInfo': {'name': 'acceptance', 'version': '0'}})
            mcp.send('notifications/initialized', notify=True)
            f9(a, 'nouser.', mcp)
        finally:
            mcp.close()


def bad_token(out, dev, env, sink, config):
    with fixture_token(config) as args:
        with app(out, 'badtoken', dev, env, sink, args=args) as a:
            a.until(lambda s: s.get('framesPainted', 0) > 0, 30)
            time.sleep(5)
            f3(a, 'badtoken.')
            f4(a, 'badtoken.')


def run(out, mode):
    os.makedirs(os.path.join(ROOT, 'target', 'tmp'), exist_ok=True)
    before, bins = real_state(), os.path.join(out, 'bin')
    build = gate.load(out, 'build.json') or {}
    build['sha256'] = {b: sha(os.path.join(bins, b)) for b in sorted(os.listdir(bins))}
    gate.save(out, 'build.json', build)
    errors = {}
    with cs.session(out, 'bramley-gate') as (sock, sink, _):
        for phase, go in phases(out, mode, bins, sock, sink):
            try:
                print(f'{time.strftime("%H:%M:%S")} {phase}', file=sys.stderr, flush=True)
                if not phase.startswith('reference') and guard_fired(out):
                    raise RuntimeError(f'skipped: the guard fired in {guard_fired(out)}')
                go()
            except Exception as e:
                errors[phase] = f'{type(e).__name__}: {e}'.replace(ROOT, '.').replace(os.path.expanduser('~'), '~')[:300]
            gate.save(out, 'errors.json', errors)
    gate.save(out, 'realstate.json', {'before': before, 'after': real_state()})
    shutil.rmtree(RUNTIME, ignore_errors=True)
    leak_grep(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out')
    ap.add_argument('--mode', choices=('counted', 'llvmpipe', 'controls'), default='counted')
    ap.add_argument('--preflight', action='store_true')
    a = ap.parse_args()
    if a.preflight:
        found = preflight()
        print('\n'.join(['refusing: other GPU clients are running:'] + found) if found else 'preflight: no other weston, bramley or GPU client')
        sys.exit(2 if found else 0)
    run(a.out, a.mode)


if __name__ == '__main__':
    main()
