import json, os, re, time

FIELDS = ('position', 'applied', 'seq', 'skippedMs', 'sentMs', 'duration', 'state')
ID = re.compile(r'[A-Za-z0-9._-]+')
STEP_S, BACK_TO_S, NATURAL_S, SHORT_REACH_S, LONG_REACH_S, ENTRY_REACH_S = 0.05, 280.0, 15.0, 150.0, 150.0, 143.0
PLAY_TIMEOUT_S, REACH_TIMEOUT_S, SEEK_TIMEOUT_S, ENTRY_TIMEOUT_S, MAX_TRACE_EVENTS = 60, 400, 90, 240, 400
TRACE_MARKER = b'bramley-unshipped-feature:engine/trace'
TRACE_LINE = re.compile(r'TRACE (\d+\.\d+) (\S.*)$')
TRACE_KINDS = (('seekToTime', r'mark step seek (\w+)$'), ('fetch', r'call \S+ (?:fetch|open) (\w+) (\S+)$'), ('fetch', r'(?:resolve|done) \S+ (\d+)\b'),
               ('append', r'call SourceBuffer#\d+ appendBuffer (\S+)$'), ('appendThrow', r'throw SourceBuffer#\d+ appendBuffer (\w+)$'), ('remove', r'call SourceBuffer#\d+ remove (\S*)$'),
               ('currentTime', r'set \w+#\d+ currentTime (\S+)$'), ('cdm', r'cdm-event keys=\d+ (\{.*\})$'))


def probe_ids(env=os.environ, names=('LONG', 'SHORT')):
    ids = {n: env.get(f'BRAMLEY_PROBE_{n}', '') for n in names}
    if not all(ID.fullmatch(i) for i in ids.values()) or len(set(ids.values())) < len(ids):
        raise RuntimeError(f'BRAMLEY_PROBE_{" and BRAMLEY_PROBE_".join(names)} must be different item ids')
    return ids


def wait(a, pred, timeout, what, clock, sleep):
    end = clock() + timeout
    while clock() < end:
        s = a.snap()
        if pred(s):
            return s
        sleep(STEP_S)
    raise RuntimeError(f'timed out waiting for {what}')


def applied(s, reply):
    return (s.get('applied') or 0) >= (reply.get('seq') or 1 << 62)


def parse_trace(lines):
    found = []
    for line in lines:
        m = TRACE_LINE.search(line)
        for kind, pattern in TRACE_KINDS if m else ():
            hit = re.match(pattern, m[2])
            if hit:
                detail = json.loads(hit[1]).get('type') if kind == 'cdm' else ' '.join(hit.groups() + tuple(re.findall(r'content-length=\d+', m[2])))
                found.append([float(m[1]), kind, detail])
                break
    base = next((t for t, kind, d in found if kind == 'seekToTime'), found[0][0] if found else 0)
    return [[round(t - base, 1), kind, detail] for t, kind, detail in found[:MAX_TRACE_EVENTS]]


class TraceLog:
    def __init__(self, path):
        self.path = path

    def mark(self):
        return os.path.getsize(self.path)

    def lines(self, start, end):
        with open(self.path, 'rb') as f:
            f.seek(start)
            return f.read(end - start).decode(errors='replace').splitlines()


class Run:
    def __init__(self, d, a, ids, ev, clock, sleep, cap, trace=None):
        self.d, self.a, self.ids, self.ev, self.clock, self.sleep, self.cap, self.trace = d, a, ids, ev, clock, sleep, cap, trace
        self.names = {v: k for k, v in ids.items()}

    def wait_for(self, pred, timeout, what):
        return wait(self.a, pred, timeout, what, self.clock, self.sleep)

    def spend(self, *args):
        if len(self.d.PACER.actions) >= self.cap:
            raise RuntimeError(f'refusing: {self.cap} license actions already spent')
        if args[0] == 'seek':
            self.d.PACER.take('seek-to-end')
        return self.a.cli(*args)

    def play(self, name, duration_ok, reach):
        reply = self.spend('play', self.ids[name])[1]
        s = self.wait_for(lambda s: s.get('itemId') == self.ids[name] and s.get('state') == 'Playing' and applied(s, reply) and s.get('duration'), PLAY_TIMEOUT_S, f'{name} to play')
        if not duration_ok(s['duration']):
            raise RuntimeError(f'{name} has duration {s["duration"]}, which is not the expected item')
        self.ev['started'] = True
        self.d.note_names(self.a.cli('now')[1], 'library')
        self.wait_for(lambda s: s.get('itemId') == self.ids[name] and (s.get('position') or 0) >= reach(s['duration']), REACH_TIMEOUT_S, f'{name} to reach its start position')

    def arm(self, name):
        g, clock, sleep = self.d.gate, self.clock, self.sleep
        start = clock()
        rows, next_tick = [], [start]

        def collect(until):
            while next_tick[0] <= until and clock() <= until:
                sleep(max(0.0, next_tick[0] - clock()))
                s = self.a.snap()
                rows.append(dict({k: s.get(k) for k in FIELDS}, t=clock(), itemId=self.names.get(s.get('itemId'), 'other' if s.get('itemId') else None)))
                next_tick[0] += g.PROBE_POLL_S
        collect(start + g.PROBE_PRE_S - g.PROBE_POLL_S / 2)
        sleep(max(0.0, start + g.PROBE_PRE_S - clock()))
        duration = rows[-1]['duration']
        if not duration:
            raise RuntimeError(f'arm {name}: no duration before the seek')
        target, logged = round(duration - 10, 1), self.trace and self.trace.mark()
        code, reply, _ = self.spend('seek', str(target))
        t_seek = self.a.last_t
        self.ev['arms'][name] = {'target': target, 'exit': code, 'tSeek': t_seek, 'seq': reply.get('seq'), 'rows': rows}
        collect(t_seek + g.PROBE_POST_S + g.PROBE_POLL_S / 2)
        if self.trace:
            self.ev['arms'][name]['trace'] = parse_trace(self.trace.lines(logged, self.trace.mark()))


def sequence(d, a, ids, ev, clock=time.time, sleep=time.sleep):
    run = Run(d, a, ids, ev, clock, sleep, d.gate.PROBE_ACTIONS)
    long_ok = lambda dur: abs(dur - d.gate.PROBE_LONG_S) <= d.gate.PROBE_LONG_TOL_S
    d.PACER.room(run.cap)
    run.play('LONG', long_ok, lambda dur: LONG_REACH_S)
    run.arm('A')
    if not (lambda s: s.get('itemId') == ids['LONG'] and s.get('state') == 'Playing')(a.snap()):
        ev['replayed'] = True
        run.play('LONG', long_ok, lambda dur: 0.0)
    back = a.cli('seek', str(BACK_TO_S))[1]
    run.wait_for(lambda s: applied(s, back) and s.get('state') == 'Playing' and (s.get('position') or 0) >= BACK_TO_S + NATURAL_S, SEEK_TIMEOUT_S, 'natural play after the seek back')
    run.arm('B')
    run.play('SHORT', lambda dur: dur < d.gate.FULL_SONG_S, lambda dur: min(SHORT_REACH_S, dur - 40))
    run.arm('C')


def sequence2(d, a, ids, ev, clock=time.time, sleep=time.sleep, trace=None):
    g, run = d.gate, Run(d, a, ids, ev, clock, sleep, d.gate.PROBE2_ACTIONS, trace)
    tail = d.ALBUM[d.ALBUM.index(ids['PRED']):] if ids['PRED'] in d.ALBUM else []
    if tail[:2] != [ids['PRED'], ids['LONG']] or len(tail) < 3:
        raise RuntimeError('PRED and LONG must be consecutive items of the acceptance album, with one more item after LONG')
    d.PACER.room(run.cap)
    run.play('PRED', lambda dur: abs(dur - g.PROBE2_PRED_S) <= g.PROBE_LONG_TOL_S, lambda dur: 0.0)
    if trace and not any('trace installed' in line for line in trace.lines(0, trace.mark())):
        raise RuntimeError('the engine did not install the trace')
    queued = [a.cli('queue', 'add', i)[1] for i in tail[1:]][-1]
    seen = run.wait_for(lambda s: applied(s, queued), SEEK_TIMEOUT_S, 'the queue to settle')
    code, sought, _ = run.spend('seek', str(round(seen['duration'] - 10, 1)))
    entry = {'from': run.names.get(seen.get('itemId')), 'seekSeq': sought.get('seq'), 'seqAtEntry': None}

    def entered(s):
        if s.get('itemId') != ids['LONG']:
            return False
        entry['seqAtEntry'] = entry['seqAtEntry'] if entry['seqAtEntry'] is not None else s.get('seq')
        return s.get('state') == 'Playing' and (s.get('position') or 0) >= ENTRY_REACH_S
    run.wait_for(entered, ENTRY_TIMEOUT_S, 'LONG to start by itself')
    ev['entry'] = dict(entry, item='LONG')
    d.PACER.note('item-change')
    if abs((a.snap().get('duration') or 0) - g.PROBE_LONG_S) > g.PROBE_LONG_TOL_S:
        raise RuntimeError('LONG has an unexpected duration')
    run.arm('A')
    d.PACER.note('item-change', sum(1 for x, y in zip(ev['arms']['A']['rows'], ev['arms']['A']['rows'][1:]) if x['itemId'] and y['itemId'] and x['itemId'] != y['itemId']))
    run.play('LONG', lambda dur: abs(dur - g.PROBE_LONG_S) <= g.PROBE_LONG_TOL_S, lambda dur: LONG_REACH_S)
    run.arm('F')
    run.play('SHORT', lambda dur: dur < g.FULL_SONG_S, lambda dur: min(SHORT_REACH_S, dur - 40))
    run.arm('C')


def session(d, out, binary, env, sink):
    ids = probe_ids()
    with d.app(out, 'probe', binary, env, sink) as a:
        a.until(lambda s: s.get('framesPainted', 0) > 0 and ((s.get('texturesUploaded') or 0) >= d.gate.COVERS or s.get('coversFailed')), 60)
        time.sleep(5)
        ev = {'t0': time.time(), 'started': False, 'arms': {}}
        try:
            sequence(d, a, ids, ev)
        finally:
            ev['actions'] = len(d.PACER.actions)
            d.evidence(out, 'probe.json', ev, a)


def session2(d, out, binary, env, sink):
    ids = probe_ids(names=('PRED', 'LONG', 'SHORT'))
    with open(binary, 'rb') as image:
        if TRACE_MARKER not in image.read():
            raise RuntimeError('the binary is not a trace build')
    with d.app(out, 'probe', binary, env, sink) as a:
        a.until(lambda s: s.get('framesPainted', 0) > 0 and ((s.get('texturesUploaded') or 0) >= d.gate.COVERS or s.get('coversFailed')), 60)
        time.sleep(5)
        trace = TraceLog(a.log.name)
        ev = {'t0': time.time(), 'started': False, 'arms': {}}
        try:
            sequence2(d, a, ids, ev, trace=trace)
        finally:
            ev['actions'] = len(d.PACER.actions)
            d.evidence(out, 'probe.json', ev, a)
