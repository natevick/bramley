import argparse, contextlib, glob, hashlib, http.server, importlib.util, json, os, queue, re, shutil, signal, socket, subprocess, sys, threading, time

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
HOLE_BAR_S = 20
NAMES = {'library': set(), 'catalog': set()}
COUNTERS = ('sentMs', 'skippedMs', 'decryptedMs', 'readinessDips', 'outputUnderflows', 'decryptFails', 'drainsAcked', 'drainTimeouts')
SNAP_KEYS = ('pid', 'state', 'position', 'duration', 'itemId', 'banner', 'engineStarted', 'texturesUploaded', 'coversFailed', 'framesPainted',
             'glRenderer', 'applied', 'seq', 'engine', 'decodedBytesKept') + COUNTERS
LICENSE_STATE = f'/run/user/{os.getuid()}/bramley-license-actions.json'
TICK, RUNTIME, CPU_EVERY_S, COTENANT_EVERY_S = os.sysconf('SC_CLK_TCK'), os.path.join(ROOT, 'target', 'x'), 10, 5
PACER = gate.Pacer()
ALBUM_ID, STOP_S, HOLD_S, BALLAST_TRIGGER = '6781024026', 3, 0.3, 'ballast.trigger'


def note_names(found, kind='catalog'):
    if isinstance(found, dict):
        NAMES[kind].update(v for k, v in found.items() if k in ('name', 'title', 'artist', 'album') and isinstance(v, str) and len(v) >= 4)
        [note_names(v, kind) for v in found.values()]
    elif isinstance(found, list):
        [note_names(v, kind) for v in found]


def leak_grep(out):
    tokens = [open(os.path.join(cs.REAL_CONFIG, f)).read().strip() for f in ('developer-token', 'music-user-token') if os.path.exists(os.path.join(cs.REAL_CONFIG, f))]
    fragments = [t[i:i + 24] for t in tokens for i in (0, len(t) // 2, len(t) - 24)]
    with contextlib.suppress(Exception):
        note_names(s9.every('/v1/me/library/playlists', tokens)[0], 'library')
        playlist, _ = largest_playlist(cs.REAL_CONFIG)
        note_names(s9.every(f'/v1/me/library/playlists/{playlist}/tracks', tokens)[0], 'library')
        note_names(s9.api(f'/v1/catalog/us/albums/{ALBUM_ID}/tracks', tokens), 'catalog')
    try:
        words = {w.strip().lower() for w in open('/usr/share/dict/words')}
    except OSError:
        words = set()
    allowed = gate.allowlisted()
    exempt = {n for n in NAMES['library'] | NAMES['catalog'] if gate.leak_exempt(n, words, allowed)}
    private, every = sorted(NAMES['library'] - exempt) + fragments, sorted((NAMES['library'] | NAMES['catalog']) - exempt) + fragments
    git = lambda *args: subprocess.run(['git', *args], cwd=ROOT, capture_output=True, text=True).stdout
    repo = '\n'.join([line for line in git('diff', 'main...HEAD').splitlines() if line.startswith('+')] + [git('log', 'main..HEAD', '--format=%B')]).lower()
    tree_hits = sum(len(git('grep', '-n', '-i', '-w', '-F', n, 'HEAD', '--', '.').splitlines()) for n in private)
    count = gate.count_names

    def walk():
        files = []
        for d, dirs, names in os.walk(out):
            dirs[:] = [x for x in dirs if x not in ('bin', 'empty')]
            files += [open(os.path.join(d, f), errors='replace').read() for f in names if not f.endswith('.png')]
        return files, count('\n'.join(files).lower(), every)

    files, run_hits = walk()
    hits = {'repo': count(repo, private) + tree_hits, 'run': run_hits}
    plant = os.path.join(out, 'leak-plant.txt')
    planted_run = False
    if private:
        with os.fdopen(os.open(plant, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), 'w') as f:
            f.write(private[0])
        try:
            planted_run = walk()[1] == run_hits + 1
        finally:
            os.remove(plant)
    gate.save(out, 'leakgrep.json', {'libraryNames': len(NAMES['library']), 'catalogNames': len(NAMES['catalog']), 'exempt': len(exempt), 'exemptByAllowlist': len(allowed & {gate.hashlib.sha256(n.lower().encode()).hexdigest() for n in exempt}),
                                     'tokenFragments': len(fragments), 'runFiles': len(files), 'treeHits': tree_hits, 'hits': hits,
                                     'plantedRun': planted_run, 'plantedRepo': gate.repo_plant(repo, private, count(repo, private))})


class Cotenants(threading.Thread):
    def __init__(self):
        super().__init__(daemon=True)
        self.stop, self.polls, self.seen = False, 0, []
        self.standing = sorted(pid for pid, (comm, libs) in gate.gpu_processes().items() if libs and comm in gate.STANDING_GPU_CLIENTS and not ours(pid))

    def run(self):
        while not self.stop:
            found = [f for f in gate.preflight(ours, set(self.standing)) if os.path.exists(f'/proc/{f.split()[0]}')]
            self.polls += 1
            if found:
                self.seen.append([round(time.time(), 1), found])
            time.sleep(COTENANT_EVERY_S)


def ours(pid):
    while pid > 1:
        if pid == os.getpid():
            return True
        try:
            pid = int(open(f'/proc/{pid}/stat').read().rsplit(')', 1)[1].split()[1])
        except (OSError, ValueError, IndexError):
            return False
    return False


def guard_fired(out):
    return sorted(f for f in os.listdir(out) if f.endswith('.guard.json') and f != 'guardtest.guard.json' and (gate.load(out, f) or {}).get('killed'))


def host_memory():
    with open('/proc/meminfo') as f:
        fields = {k: int(v) for k, v in (line.replace(':', '').split()[:2] for line in f)}
    return {k: round(fields[f] / 1024) for k, f in (('available', 'MemAvailable'), ('free', 'MemFree'), ('swapFree', 'SwapFree'), ('shmem', 'Shmem'),
                                                    ('activeFile', 'Active(file)'), ('inactiveFile', 'Inactive(file)'))}


def mem(pid):
    procs = [pid] + cs.children(pid)
    rolls = [cs.rollup(p) for p in procs]
    total = lambda *keys: round(sum(r.get(k, 0) for r in rolls for k in keys) / 1024, 2)
    split = [{'pid': p, 'shared': round((r.get('Shared_Clean', 0) + r.get('Shared_Dirty', 0)) / 1024, 2), 'private': round((r.get('Private_Clean', 0) + r.get('Private_Dirty', 0)) / 1024, 2)}
             for p, r in zip(procs, rolls)]
    return {'pss': total('Pss'), 'uss': total('Private_Clean', 'Private_Dirty'), 'rss': total('Rss'), 'procs': len(rolls), 'split': split, 'anon': total('Anonymous'), 'swap': total('Swap'), 'vmstat': gate.vmstat(), 'host': host_memory()}


def ticks(pid):
    total = 0
    for p in [pid] + cs.children(pid):
        fields = open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()
        total += sum(int(f) for f in fields[11:15])
    return total


def cold_cache(env):
    for sub in ('art', 'api'):
        shutil.rmtree(os.path.join(env['XDG_CACHE_HOME'], 'bramley', sub), ignore_errors=True)


class App:
    def __init__(self, out, name, binary, env, sink, args=(), wrap=(), status=True):
        self.out, self.name, self.binary, self.env, self.sink = out, name, binary, env, sink
        self.modes = None
        self.st = os.path.join(out, f'{name}.status.json') if status else None
        cs.assert_isolated(env)
        cold_cache(env)
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

    def cli(self, *args, paced=False):
        if args[:1] in (('play',), ('queue',)):
            self.safe_to_play()
        if gate.license_kind(args) and not paced:
            PACER.take(gate.license_kind(args))
        self.last_t = t = time.time()
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
                return self.noted(s), time.time()
            time.sleep(step)
        return self.noted(s), None

    def noted(self, s):
        if self.modes is None and all(s.get(k) is not None for k in gate.OFF_MODES):
            self.modes = {k: s[k] for k in gate.OFF_MODES}
        return s

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
                                                        'seen': sorted(map(str, self.guard.seen)), 'log': self.guard.log, 'stalls': self.guard.stalls, 'modes': self.modes})


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
    ev = {'session': a.name, 't0': time.time(), 'before': a.snap().get('state'), 'binS': CPU_EVERY_S, 'procs': []}
    rows, last = [], ticks(a.pid)
    for _ in range(gate.CPU_SAMPLES):
        time.sleep(CPU_EVERY_S)
        now = ticks(a.pid)
        rows.append(round(100 * (now - last) / TICK / CPU_EVERY_S, 2))
        ev['procs'].append(1 + len(cs.children(a.pid)))
        last = now
    ev.update(cpu=rows, mean=round(sum(rows) / len(rows), 3), after=a.snap().get('state'), started=state == 'Playing')
    evidence(a.out, f'{pre}{name}.json', ev, a)


def applied(reply, effect, same_engine=True):
    return lambda s: (s.get('engine') == reply.get('engine') or not same_engine) and (s.get('applied') or 0) >= (reply.get('seq') or 1 << 62) and effect(s)


def advancing(a, ready, timeout):
    prev, end, s = None, time.time() + timeout, {}
    while time.time() < end:
        s = a.snap()
        pos = s.get('position') or 0
        if not ready(s):
            prev = None
        elif prev is not None and pos - prev > gate.ADVANCE_EPS_S:
            return s, time.time()
        elif prev is None or pos < prev:
            prev = pos
        time.sleep(0.005)
    return s, None


def counters(s, t=None):
    return dict({k: s.get(k) for k in COUNTERS}, t=round(t or time.time(), 3), position=s.get('position'))


def judged(a, song, base, heard, ev):
    s, _ = a.until(lambda s: s.get('itemId') == song and (s.get('position') or 0) > gate.F4_PLAYED_S + 2, 60, 0.25)
    ev['judged'] = [{'base': base, 'heard': heard, 'end': counters(s), 'itemId': s.get('itemId'), 'state': s.get('state'), 'banner': s.get('banner'), 'duration': s.get('duration')}]
    note_names(a.cli('now')[1])


def play_song(a, song, reps, ev):
    ev.update(song=song, reps=[], started=False)
    for _ in range(reps):
        a.safe_to_play()
        PACER.take('play')
        base = counters(a.cli('now')[1])
        code, reply, _ = a.cli('play', song, paced=True)
        ready = applied(reply, lambda s: s.get('itemId') == song and s.get('state') == 'Playing', same_engine=False)
        s, seen = advancing(a, ready, 15)
        ev['reps'].append({'exit': code, 'audioMs': seen and round((seen - a.last_t) * 1000), 'banner': s.get('banner')})
        ev['started'] |= seen is not None
    if ev['started']:
        judged(a, song, base, counters(s, seen), ev)


def f4(a, pre):
    ev = {'t0': time.time(), 'information': 'warm replays; F4 is judged on f4.json (cold launches)'}
    play_song(a, SONG, gate.REPS, ev)
    evidence(a.out, f'{pre}f4warm.json', ev, a)


def f4_cold(out, binary, env, sink, pre='', args=()):
    ev = {'t0': time.time(), 'song': SONG, 'reps': [], 'started': False, 'renderers': [], 'judged': []}
    for i in range(gate.REPS):
        with app(out, f'{pre}f4cold-{i}', binary, env, sink, args=args) as a:
            a.until(lambda s: s.get('framesPainted', 0) > 0 and ((s.get('texturesUploaded') or 0) >= gate.COVERS or s.get('coversFailed') or s.get('banner')), 60)
            one = {'song': SONG}
            play_song(a, SONG, 1, one)
            ev['reps'] += one['reps']
            ev['started'] |= one['started']
            ev['renderers'] += a.renderers()
            ev['judged'] += one.get('judged', [])
            ev.setdefault('sessions', []).append(a.name)
            ev['t1'] = time.time()
    gate.save(out, f'{pre}f4.json', ev)


def held(a, ready, timeout):
    mark, end, s = None, time.time() + timeout, {}
    while time.time() < end:
        s, now = a.snap(), time.time()
        pos = s.get('position')
        if not ready(s):
            mark = None
        elif mark is None or pos != mark[1]:
            mark = (now, pos)
        elif now - mark[0] >= HOLD_S:
            return s, mark[0]
        time.sleep(0.005)
    return s, None


def f5(a, pre):
    ev, args = {'t0': time.time(), 'rounds': [], 'started': False}, {
        'pause': ('pause',), 'seek': ('seek', str(gate.F5_SEEK_S)), 'resume': ('play',), 'next': ('next',), 'prev': ('prev',)}
    items = {'next': SECOND, 'prev': SONG}
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
            code, reply, _ = a.cli(*args[cmd])
            t = a.last_t
            ms = lambda seen: seen and round((seen - t) * 1000)
            _, acked = a.until(applied(reply, lambda s: True), 5, 0.005)
            row[cmd] = {'exit': code, 'appliedMs': ms(acked)}
            if cmd == 'pause':
                s, seen = held(a, applied(reply, lambda s: s.get('state') == 'Paused'), 5)
            elif cmd == 'seek':
                s, seen = a.until(applied(reply, lambda s: abs((s.get('position') or 0) - gate.F5_SEEK_S) <= gate.F5_POSITION_S), 5, 0.005)
            elif cmd == 'resume':
                s, seen = advancing(a, applied(reply, lambda s: s.get('state') == 'Playing' and (s.get('position') or 0) >= gate.F5_SEEK_S - gate.F5_POSITION_S), 5)
            else:
                s, seen = a.until(applied(reply, lambda s: s.get('itemId') == items[cmd]), 5, 0.005)
                _, heard = advancing(a, lambda s: s.get('itemId') == items[cmd] and s.get('state') == 'Playing', 10)
                row[cmd]['advancingMs'] = ms(heard)
            row[cmd].update(effectMs=ms(seen), position=s.get('position'))
        ev['rounds'].append(row)
    evidence(a.out, f'{pre}f5.json', ev, a)


def transition(a, ids, at, start, cut=False):
    s, ok = a.until(lambda s: s.get('itemId') == ids[at] and s.get('state') == 'Playing' and (s.get('duration') or 0) > 30 and (s.get('position') or 0) > start + 1, 40)
    if not ok:
        return {'reached': False, 'pass': False}
    duration, target = s['duration'], round(s['duration'] - 20, 1)
    PACER.take('seek-to-end')
    reply = a.cli('seek', str(target))[1]
    res = {'reached': True, 'duration': duration, 'target': target, 'start': start, 'base': None, 'flip': None, 'end': None, 'cut': False, 'samples': []}
    end = time.time() + 45
    while time.time() < end and (res['end'] is None or res['end']['t'] < res['flip']['t'] + F6_WINDOW_S):
        s, now = a.snap(), time.time()
        pos = s.get('position') or 0
        res['samples'].append([round(now, 3), pos])
        if res['base'] is None and applied(reply, lambda s: True)(s):
            res['base'] = counters(s, now)
        if res['flip'] is None and res['base'] and s.get('itemId') == ids[at + 1]:
            res['flip'] = counters(s, now)
        if res['flip']:
            res['end'] = counters(s, now)
        if cut and res['base'] and not res['flip'] and not res['cut'] and s.get('itemId') == ids[at] and pos - start >= duration - CUT_BEFORE_END_S:
            a.cli('next')
            res['cut'] = True
        time.sleep(0.02)
    return res


def queue_up(a, ids, ev):
    sent = lambda r: (ev['exits'].append(r[0]), ev['replies'].append([r[1].get('seq'), r[1].get('applied')]))
    ev.update(exits=[], replies=[])
    sent(a.cli('play', ids[0]))
    ev['started'] = a.until(lambda s: s.get('itemId') == ids[0] and s.get('state') == 'Playing' and (s.get('position') or 0) > 1, 30)[1] is not None
    [sent(a.cli('queue', 'add', i)) for i in ids[1:]]
    a.until(lambda s: (s.get('queueLength') or 0) >= len(ids), 10)
    return ev['started']


def f6(a, pre, cut=False):
    ids, ev = ALBUM[2:6], {'t0': time.time()}
    queue_up(a, ids, ev)
    ev['transitions'] = []
    for i in range(3):
        ev['transitions'].append(transition(a, ids, i, 0.0, cut and i == 2))
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
                PACER.take('seek-to-end')
                a.cli('seek', str(max(0.0, (a.snap().get('duration') or 30) - 5)))
        time.sleep(0.1)
    evidence(a.out, f'{pre}f8.json', ev, a)


def r3(a, pre, loop=False):
    a.safe_to_play()
    ev = {'pid': a.pid, 't0': time.time(), 'seekTimes': [], 'underflowEvents': [], 'changes': [], 'stalls': [], 'refills': [], 'soakS': SOAK_S}
    PACER.room(gate.R3_RESERVE)
    queue_up(a, ALBUM, ev)
    seeks, starts, sought, start = [0], {}, set(), time.time()
    underflows, queued, cur, stalls = [a.snap().get('outputUnderflows') or 0], [len(ALBUM)], [None], gate.StallWatch()
    ev['stalls'] = stalls.stalls

    def pace():
        s = a.snap()
        item, pos, dur = s.get('itemId'), s.get('position') or 0, s.get('duration') or 0
        count = s.get('outputUnderflows')
        if count is not None and count > underflows[0]:
            ev['underflowEvents'].append({'t': round(time.time(), 2), 'itemId': item, 'position': pos, 'outputUnderflows': count,
                                          'drainsAcked': s.get('drainsAcked'), 'lastSeekT': (ev['seekTimes'] or [None])[-1]})
        underflows[0] = max(underflows[0], count or 0)
        stalls.update(time.time(), s.get('state'), item, pos)
        if item and s.get('state') == 'Playing' and item != cur[0]:
            cur[0] = item
            ev['changes'].append([round(time.time(), 2), item])
            if len(ev['changes']) > 1:
                PACER.note('item-change')
            starts[len(ev['changes'])] = pos
        at = len(ev['changes'])
        if gate.needs_refill(loop, queued[0], at):
            [a.cli('queue', 'add', i) for i in ALBUM]
            queued[0] += len(ALBUM)
            ev['refills'].append(round(time.time(), 2))
        if at in starts and at not in sought and item == cur[0] and dur > SOAK_TRACK_S + 15 and pos - starts[at] >= SOAK_TRACK_S:
            sought.add(at)
            ev['seekTimes'].append(round(time.time(), 2))
            a.cli('seek', str(gate.seek_target(dur)))
            ev['seekTimes'].append(round(time.time(), 2))
            seeks[0] += 1
        return {'m': round((time.time() - start) / 60, 3), 'seeks': seeks[0]}

    rows = []
    try:
        while time.time() - start < SOAK_S + SOAK_EVERY_S:
            rows += sample(a, 1, 0, pace)
            for _ in range(SOAK_EVERY_S):
                time.sleep(1)
                pace()
    finally:
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
        if ev['started'] and 'ballastMiB' in extra:
            r1['baseSamples'] = sample(a, gate.R1_SAMPLES)
            r1['ballastTouched'] = touch_ballast(a, extra['ballastMiB'])
        if ev['started']:
            r1['samples'] = sample(a, gate.R1_SAMPLES)
        r1['gpuAfter'] = cs.gpu()
    finally:
        gate.save(a.out, f'{name}.r1.json', r1)


def touch_ballast(a, mib):
    trigger = os.path.join(a.out, BALLAST_TRIGGER)
    with open(trigger + '.part', 'w') as f:
        f.write(str(mib))
    os.replace(trigger + '.part', trigger)
    return a.until(lambda s: s.get('ballastMiB') == mib, 30)[0].get('ballastMiB')


def cpu_session(out, binary, env, sink):
    with app(out, 'cpu', binary, env, sink, status=False) as a:
        a.until(lambda s: (s.get('texturesUploaded') or 0) >= gate.COVERS, 60)
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
             'cut': lambda a, p: f6(a, p, cut=True), 'stall': stall, 'soak': lambda a, p: r3(a, p, loop=True)}[step](a, pre)


def stall(a, pre):
    ev = {'t0': time.time()}
    play_song(a, SONG, 1, ev)
    a.until(lambda s: (s.get('position') or 0) > 15, 30)
    ev['before'] = dict(counters(a.snap()), itemId=a.snap().get('itemId'))
    os.kill(a.pid, signal.SIGSTOP)
    try:
        time.sleep(STOP_S)
    finally:
        os.kill(a.pid, signal.SIGCONT)
    time.sleep(10 - STOP_S)
    ev.update(after=dict(counters(a.snap()), itemId=a.snap().get('itemId')), stoppedS=STOP_S)
    evidence(a.out, f'{pre}stall.json', ev, a)


def guardtest(out, binary, env, sink):
    wrong = cs.unique_sink('bramley-gate-wrong', out)
    pw = cs.null_sink(wrong)
    ev = {'t0': time.time(), 'defaultBefore': cs.default_sink()}
    try:
        with app(out, 'guardtest', binary, dict(env, PULSE_SINK=wrong), sink) as a:
            a.until(lambda s: (s.get('texturesUploaded') or 0) >= gate.COVERS or s.get('coversFailed'), 60)
            ev['wrongSinkPresent'] = cs.sink_exists(wrong) and cs.default_sink() not in (None, wrong, sink)
            if not ev['wrongSinkPresent']:
                raise RuntimeError('the second null sink is not present, or is the default; refusing')
            a.cli('play', SONG)
            cs.wait_for(lambda: a.guard.killed, 30, 'the guard to kill the app')
            ev.update(killed=a.guard.killed['why'], appGone=a.p.wait(10) is not None, wrongSinkSeen=wrong in map(str, a.guard.seen))
    finally:
        os.killpg(pw.pid, signal.SIGTERM)
        ev['defaultAfter'] = cs.default_sink()
        gate.save(out, 'guardtest.json', ev)


def f1(out, binary, env, sink, pre=''):
    ev = {'starts': []}
    for i in range(gate.REPS):
        with app(out, f'{pre}f1-{i}', binary, env, sink) as a:
            s, seen = a.until(lambda s: s.get('framesPainted', 0) > 0, 10, 0.002)
            listed, _ = a.until(lambda s: ((s.get('library') or {}).get('playlists') or {}).get('loaded'), 20)
            ev['starts'].append({'ms': seen and round((seen - a.t_exec) * 1000), 'route': listed.get('route'), 'playlists': ((listed.get('library') or {}).get('playlists') or {}).get('loaded')})
            ev.setdefault('renderers', []).append(s.get('glRenderer'))
            ev.setdefault('sessions', []).append(a.name)
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
    for _ in range(gate.REPS):
        a.safe_to_play()
        PACER.take('mcp-play')
        base = counters(a.cli('now')[1])
        t = time.time()
        err, said = mcp.tool('play', {'id': SONG})
        reply = {'seq': a.cli('now')[1].get('seq')}
        s, seen = advancing(a, applied(reply, lambda s: s.get('itemId') == SONG and s.get('state') == 'Playing', same_engine=False), 15)
        ev['reps'].append({'error': err, 'audioMs': seen and round((seen - t) * 1000)})
        ev['started'] |= seen is not None
    if ev['started']:
        judged(a, SONG, base, counters(s, seen), ev)
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
        ev['reps'].append({'exit': code, 'wallMs': wall, 'songs': len(songs), 'uiMs': seen and round((seen - t) * 1000), 'error': str(reply.get('error') or reply.get('refused'))[:120]})
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
        f7(a, playlist)
        before = a.snap()
        ev = {'t0': time.time(), 'pidBefore': before.get('pid'), 'bannerBefore': before.get('banner')}
        code, reply, _ = a.cli('play', BAD_ID)
        a.until(lambda s: s.get('banner') not in (None, ev['bannerBefore']), 20)
        time.sleep(3)
        ev.update(exit=code, refused=reply.get('refused') or reply.get('error'), banner=a.snap().get('banner'), alive=a.p.poll() is None and a.cli('now')[1].get('pid') == ev['pidBefore'])
        evidence(out, 'f10-badid.json', ev, a)
    return playlist


def f7(a, playlist):
    exits = [a.cli('open', f'playlist:{playlist}')[0], a.cli('scroll', 'end')[0]] if playlist else []
    done = lambda lib: (lib.get('tracks') or {}).get('loaded') == (lib.get('tracks') or {}).get('total') == lib.get('shownTo') and lib.get('artTextures')
    s, _ = a.until(lambda s: s.get('route') == 'Playlist' and done(s.get('library') or {}), 120, 0.5)
    lib, tracks, mcp = s.get('library') or {}, (s.get('library') or {}).get('tracks') or {}, gate.load(a.out, 'f7.mcp.json') or {}
    gate.save(a.out, 'f7.json', {'driver': 'shipped: MCP library_playlists, then `open playlist:<id>` and `scroll end` through the CLI', 'exits': exits, 'mcpTotal': mcp.get('total'),
                                 'largestTracks': mcp.get('largestTracks'), 'viewTotal': tracks.get('total'), 'endReached': bool(exits) and not any(exits) and tracks.get('loaded') == tracks.get('total') == lib.get('shownTo') and (tracks.get('pages') or 0) > 1,
                                 'artTextures': lib.get('artTextures'), 'session': a.name, 'renderers': a.renderers()})


def offline_session(out, binary, env, sink):
    resolv = os.path.join(out, 'netns-resolv.conf')
    open(resolv, 'w').write('nameserver 10.0.2.3\n')
    wrap = ('bwrap', '--dev-bind', '/', '/', '--ro-bind', resolv, '/etc/resolv.conf', '--unshare-net', '--die-with-parent', '--')
    api = socket.getaddrinfo('api.music.apple.com', 443, socket.AF_INET, socket.SOCK_STREAM)[0][4][0]
    slirp = None
    with app(out, 'offline', binary, env, sink, wrap=wrap) as a:
        ev = {'t0': time.time()}
        probe = lambda: subprocess.run(['nsenter', '-t', str(a.pid), '-U', '-n', '--preserve-credentials', 'python3', '-I', '-c',
                                        f'import socket; socket.create_connection(("{api}", 443), 5)'], capture_output=True, timeout=20).returncode == 0
        attach = lambda: subprocess.Popen(['slirp4netns', '--configure', '--mtu=65520', '--disable-host-loopback', str(a.pid), 'tap0'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        def search(key, mark, want):
            ev[f'{key}At'] = time.time()
            code, reply, ms = a.cli('search', TERM)
            ui, _ = a.until(lambda s: (s.get('search') or {}).get('state') == want and ((s.get('search') or {}).get('seq') or 0) > mark, 10)
            ev[key] = {'exit': code, 'ui': (ui.get('search') or {}).get('state'), 'error': reply.get('error'), 'wallMs': ms}
            return (ui.get('search') or {}).get('seq') or 0
        try:
            a.until(lambda s: s.get('framesPainted', 0) > 0, 30)
            time.sleep(10)
            ev['probeCold'] = probe()
            mark = search('coldSearch', 0, 'error')
            ev.update(downError=ev['coldSearch']['exit'] != 0, downUi=ev['coldSearch']['ui'], coversWhileDown=a.snap().get('texturesUploaded'), alive=a.p.poll() is None)
            ev['homeOpenedWhileDown'] = a.cli('open', 'home')[0] == 0
            slirp = attach()
            up = time.time()
            _, back = a.until(lambda s: (s.get('texturesUploaded') or 0) >= gate.COVERS, RECOVER_S, 0.5)
            ev['coversRecoveredS'], ev['probeUp'] = back and round(back - up, 1), probe()
            mark = search('afterCold', mark, 'results')
            ev.update(searchAfter=ev['afterCold']['exit'] == 0, uiAfter=ev['afterCold']['ui'])
            slirp.terminate()
            slirp.wait(5)
            ev['probeDropped'] = probe()
            mark = search('midDrop', mark, 'error')
            slirp = attach()
            time.sleep(2)
            ev['probeBack'] = probe()
            mark = search('afterDrop', mark, 'results')
            ev['dropGapS'] = round(ev['afterDropAt'] - ev['afterColdAt'], 1)
            mark = search('beforeHole', mark, 'results')
            slirp.send_signal(signal.SIGSTOP)
            mark = search('inHole', mark, 'results')
            time.sleep(3)
            slirp.send_signal(signal.SIGCONT)
            resumed, ev['holeTries'] = time.time(), []
            while time.time() - resumed < HOLE_BAR_S and not ev['holeTries'][-1:] == [{'exit': 0, 'ui': 'results'}]:
                mark = search('hole', mark, 'results')
                ev['holeTries'].append({'exit': ev['hole']['exit'], 'ui': ev['hole']['ui']})
                time.sleep(0 if ev['holeTries'][-1] == {'exit': 0, 'ui': 'results'} else 2)
            ev['holeRecoveredS'] = round(ev['holeAt'] - resumed, 1) if ev['holeTries'][-1:] == [{'exit': 0, 'ui': 'results'}] else None
            ev['aliveAtEnd'] = a.p.poll() is None and a.cli('now')[1].get('pid') == a.pid
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
        made[view] = os.path.exists(png)
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
    listing = lambda d: {f: [s.st_mtime_ns, s.st_size] for f in sorted(os.listdir(d)) for s in [os.stat(os.path.join(d, f))]} if os.path.isdir(d) else None
    token = os.path.join(cs.REAL_CONFIG, 'music-user-token')
    st = os.stat(token) if os.path.exists(token) else None
    return {'tokenMtimeNs': st and st.st_mtime_ns, 'tokenSize': st and st.st_size, 'keyringItems': cs.keyring_items('bramley'),
            'cache': listing(cs.REAL_CACHE), 'config': listing(cs.REAL_CONFIG)}


class BadSignature(http.server.BaseHTTPRequestHandler):
    token = ''

    def do_GET(self):
        body = json.dumps({'token': BadSignature.token, 'exp': int(time.time()) + 90 * 86400}).encode()
        self.send_response(200 if self.path == '/developer-token.json' else 404)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def bad_signature(token):
    head, payload, signature = token.split('.')
    return '.'.join((head, payload, signature[::-1] if signature[::-1] != signature else 'A' + signature[1:]))


@contextlib.contextmanager
def fixture_token(config):
    BadSignature.token = bad_signature(open(os.path.join(config, 'developer-token')).read().strip())
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), BadSignature)
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
    if mode == 'soak':
        yield 'soak', lambda: signed_in('soak', lambda e, c: main_session(out, shipped, e, sink, 'soak.', ('soak',)))
    elif mode == 'llvmpipe':
        yield 'f1', lambda: signed_in('f1', lambda e, c: f1(out, shipped, e, sink))
        yield 'f4', lambda: signed_in('f4', lambda e, c: f4_cold(out, shipped, e, sink))
        yield 'shipped', lambda: signed_in('shipped', lambda e, c: main_session(out, shipped, e, sink, '', ('r2', 'r1')))
    elif mode == 'controls':
        yield 'nouser', lambda: signed_out('nouser', lambda e: (f2(out, shipped, e, sink, 'nouser.'), f4_cold(out, shipped, e, sink, 'nouser.'),
                                                                 main_session(out, shipped, e, sink, 'nouser.', ('r1', 'f5', 'f6')),
                                                                 browse_nouser(out, shipped, e, sink)))
        yield 'badtoken', lambda: signed_in('badtoken', lambda e, c: bad_token(out, dev, e, sink, c))
        yield 'cut', lambda: signed_in('cut', lambda e, c: main_session(out, shipped, e, sink, 'cut.', ('cut',)))
        yield 'stall', lambda: signed_in('stall', lambda e, c: main_session(out, shipped, e, sink, 'stall.', ('stall',)))
        yield 'nostream', lambda: signed_in('nostream', lambda e, c: f4_cold(out, shipped, dict(e, PULSE_SINK=sink + '-missing'), sink, 'nostream.'))
        yield 'leak', lambda: signed_in('leak', lambda e, c: main_session(out, sabotage, e, sink, 'leak.', ('r3',)), BRAMLEY_GATE_LEAK_MIB='3')
        yield 'guardtest', lambda: signed_in('guardtest', lambda e, c: guardtest(out, shipped, e, sink))
    else:
        yield 'f1', lambda: signed_in('f1', lambda e, c: f1(out, shipped, e, sink))
        yield 'f2', lambda: signed_out('f2', lambda e: f2(out, shipped, e, sink))
        yield 'f4', lambda: signed_in('f4', lambda e, c: f4_cold(out, shipped, e, sink))
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
    trigger = os.path.join(out, BALLAST_TRIGGER)
    signed_in('ballast', lambda e, c: main_session(out, sabotage, e, sink, '', ('r2', 'r1'), {'ballastMiB': mib}), BRAMLEY_GATE_BALLAST_FILE=trigger)


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
    with fixture_token(config) as args:
        f4_cold(out, dev, env, sink, 'badtoken.', args)


def keep_actions(times):
    recent = [t for t in times if t > time.time() - gate.LICENSE_WINDOW_S]
    with open(LICENSE_STATE + '.new', 'w') as f:
        json.dump(recent, f)
    os.replace(LICENSE_STATE + '.new', LICENSE_STATE)


def prior_actions():
    try:
        return [t for t in json.load(open(LICENSE_STATE)) if t > time.time() - gate.LICENSE_WINDOW_S]
    except (OSError, ValueError):
        return []


def phase_error(e):
    if isinstance(e, s9.HarnessApiError):
        return {'kind': 'harness-api', 'code': e.code, 'path': e.path_kind}
    return f'{type(e).__name__}: {e}'.replace(ROOT, '.').replace(os.path.expanduser('~'), '~')[:300]


def run(out, mode, soak_minutes=None):
    global PACER, SOAK_S
    SOAK_S = int(soak_minutes * 60) if soak_minutes else SOAK_S
    PACER = gate.Pacer(prior_actions(), keep=keep_actions)
    os.makedirs(os.path.join(ROOT, 'target', 'tmp'), exist_ok=True)
    before, bins = real_state(), os.path.join(out, 'bin')
    build = gate.load(out, 'build.json') or {}
    build['sha256'] = {b: sha(os.path.join(bins, b)) for b in sorted(os.listdir(bins))}
    gate.save(out, 'build.json', build)
    errors, watch = {}, Cotenants()
    watch.start()
    with cs.session(out, 'bramley-gate') as (sock, sink, _):
        for phase, go in phases(out, mode, bins, sock, sink):
            try:
                print(f'{time.strftime("%H:%M:%S")} {phase}', file=sys.stderr, flush=True)
                if not phase.startswith('reference') and guard_fired(out):
                    raise RuntimeError(f'skipped: the guard fired in {guard_fired(out)}')
                go()
            except Exception as e:
                errors[phase] = phase_error(e)
            gate.save(out, 'errors.json', errors)
    watch.stop = True
    gate.save(out, 'pacing.json', {'limit': gate.LICENSE_LIMIT, 'windowS': gate.LICENSE_WINDOW_S, 'prior': PACER.prior, 'actions': PACER.actions, 'waits': PACER.waits})
    gate.save(out, 'cotenants.json', {'polls': watch.polls, 'seen': watch.seen})
    gate.save(out, 'realstate.json', {'before': before, 'after': real_state()})
    shutil.rmtree(RUNTIME, ignore_errors=True)
    leak_grep(out)


def cleanup(run):
    for d in glob.glob(os.path.join(ROOT, 'target', 'tmp', f'config-{run}-*')):
        shutil.rmtree(d, ignore_errors=True)
    path = ['--object-path', '/org/freedesktop/secrets/aliases/default', '--method', 'org.freedesktop.DBus.Properties.Get']
    listed = subprocess.run(cs.SECRETS + path + ['org.freedesktop.Secret.Collection', 'Items'], capture_output=True, text=True, timeout=10).stdout
    for item in re.findall(r"'(/org/freedesktop/secrets/collection/[^']+)'", listed):
        attrs = subprocess.run(cs.SECRETS + ['--object-path', item, '--method', 'org.freedesktop.DBus.Properties.Get', 'org.freedesktop.Secret.Item', 'Attributes'],
                               capture_output=True, text=True, timeout=10).stdout
        if re.search(rf"'service': <?'bramley-gate-{re.escape(run)}-", attrs):
            subprocess.run(cs.SECRETS + ['--object-path', item, '--method', 'org.freedesktop.Secret.Item.Delete'], capture_output=True, timeout=10)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out')
    ap.add_argument('--mode', choices=('counted', 'llvmpipe', 'controls', 'soak'), default='counted')
    ap.add_argument('--soak-minutes', type=float)
    ap.add_argument('--cleanup')
    a = ap.parse_args()
    if a.cleanup:
        return cleanup(a.cleanup)
    run(a.out, a.mode, a.soak_minutes)


if __name__ == '__main__':
    main()
