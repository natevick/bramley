import os, re, time

FIELDS = ('position', 'applied', 'seq', 'skippedMs', 'sentMs', 'duration', 'state')
ID = re.compile(r'[A-Za-z0-9._-]+')
STEP_S, BACK_TO_S, NATURAL_S, SHORT_REACH_S, LONG_REACH_S = 0.05, 280.0, 15.0, 150.0, 150.0
PLAY_TIMEOUT_S, REACH_TIMEOUT_S, SEEK_TIMEOUT_S = 60, 400, 90


def probe_ids(env=os.environ):
    ids = {'LONG': env.get('BRAMLEY_PROBE_LONG', ''), 'SHORT': env.get('BRAMLEY_PROBE_SHORT', '')}
    if not all(ID.fullmatch(i) for i in ids.values()) or ids['LONG'] == ids['SHORT']:
        raise RuntimeError('BRAMLEY_PROBE_LONG and BRAMLEY_PROBE_SHORT must be two different item ids')
    return ids


def wait(a, pred, timeout, what, clock, sleep):
    end = clock() + timeout
    while clock() < end:
        s = a.snap()
        if pred(s):
            return s
        sleep(STEP_S)
    raise RuntimeError(f'timed out waiting for {what}')


def spend(d, a, *args):
    if len(d.PACER.actions) >= d.gate.PROBE_ACTIONS:
        raise RuntimeError(f'refusing: {d.gate.PROBE_ACTIONS} license actions already spent')
    if args[0] == 'seek':
        d.PACER.take('seek-to-end')
    return a.cli(*args)


def sequence(d, a, ids, ev, clock=time.time, sleep=time.sleep):
    names = {v: k for k, v in ids.items()}
    wait_for = lambda pred, timeout, what: wait(a, pred, timeout, what, clock, sleep)
    applied = lambda s, reply: (s.get('applied') or 0) >= (reply.get('seq') or 1 << 62)

    def play(name, duration_ok, reach):
        reply = spend(d, a, 'play', ids[name])[1]
        s = wait_for(lambda s: s.get('itemId') == ids[name] and s.get('state') == 'Playing' and applied(s, reply) and s.get('duration'), PLAY_TIMEOUT_S, f'{name} to play')
        if not duration_ok(s['duration']):
            raise RuntimeError(f'{name} has duration {s["duration"]}, which is not the expected item')
        ev['started'] = True
        d.note_names(a.cli('now')[1], 'library')
        wait_for(lambda s: s.get('itemId') == ids[name] and (s.get('position') or 0) >= reach(s['duration']), REACH_TIMEOUT_S, f'{name} to reach its start position')

    def arm(name):
        start = clock()
        rows, next_tick = [], [start]

        def collect(until):
            while next_tick[0] <= until and clock() <= until:
                sleep(max(0.0, next_tick[0] - clock()))
                s = a.snap()
                rows.append(dict({k: s.get(k) for k in FIELDS}, t=clock(), itemId=names.get(s.get('itemId'), 'other' if s.get('itemId') else None)))
                next_tick[0] += d.gate.PROBE_POLL_S
        collect(start + d.gate.PROBE_PRE_S - d.gate.PROBE_POLL_S / 2)
        sleep(max(0.0, start + d.gate.PROBE_PRE_S - clock()))
        duration = rows[-1]['duration']
        if not duration:
            raise RuntimeError(f'arm {name}: no duration before the seek')
        target = round(duration - 10, 1)
        code, reply, _ = spend(d, a, 'seek', str(target))
        t_seek = a.last_t
        ev['arms'][name] = {'target': target, 'exit': code, 'tSeek': t_seek, 'seq': reply.get('seq'), 'rows': rows}
        collect(t_seek + d.gate.PROBE_POST_S + d.gate.PROBE_POLL_S / 2)

    long_ok = lambda dur: abs(dur - d.gate.PROBE_LONG_S) <= d.gate.PROBE_LONG_TOL_S
    d.PACER.room(d.gate.PROBE_ACTIONS)
    play('LONG', long_ok, lambda dur: LONG_REACH_S)
    arm('A')
    if not (lambda s: s.get('itemId') == ids['LONG'] and s.get('state') == 'Playing')(a.snap()):
        ev['replayed'] = True
        play('LONG', long_ok, lambda dur: 0.0)
    back = a.cli('seek', str(BACK_TO_S))[1]
    wait_for(lambda s: applied(s, back) and s.get('state') == 'Playing' and (s.get('position') or 0) >= BACK_TO_S + NATURAL_S, SEEK_TIMEOUT_S, 'natural play after the seek back')
    arm('B')
    play('SHORT', lambda dur: dur < d.gate.FULL_SONG_S, lambda dur: min(SHORT_REACH_S, dur - 40))
    arm('C')


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
