import importlib.util, json, os, shutil, sys, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location('gate', os.environ.get('GATE_UNDER_TEST', os.path.join(HERE, 'gate.py')))
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)
SCRATCH = os.path.join(gate.ROOT, 'target', 'tmp', f'test-gate-{os.getpid()}')
RADEONSI, LLVMPIPE, SINK, SONG = 'AMD Radeon RX 5700 XT (radeonsi, navi10)', 'llvmpipe (LLVM 21.1.8, 256 bits)', 'null-1', '6781024437'
SESSIONS = ('shipped', 'ballast', 'signedout', 'browse', 'offline', 'cpu')
OFF = {'shuffleMode': 'off', 'repeatMode': 'off'}


def weston_says(*lines):
    def write(out):
        with open(os.path.join(out, 'weston.log'), 'w') as f:
            f.writelines(f'[16:19:16.134] {line}\n' for line in lines)
    return write


def rows(n, pss, uss, rss, renderer=RADEONSI, playing=True, pid=7):
    return [{'t': 1000.0 + i, 'pid': pid, 'pss': pss, 'uss': uss, 'rss': rss, 'procs': 1, 'glRenderer': renderer, 'texturesUploaded': 50,
             'engineStarted': playing, 'state': 'Playing' if playing else 'Stopped', 'banner': None, 'position': 20.0 + i if playing else None} for i in range(n)]


def soak(n=181, uss=148.0):
    return [{'t': 1000.0 + 10 * i, 'm': round(i / 6, 3), 'pid': 7, 'procs': 1, 'pss': 170.0, 'uss': uss, 'rss': 205.0, 'texturesUploaded': 50, 'state': 'Playing',
             'banner': None, 'itemId': f'i{i // 15}', 'position': 20.0 + 10 * (i % 15), 'duration': 160.0, 'seeks': 0, 'readinessDips': 2, 'outputUnderflows': 4} for i in range(n)]


def counts(t, position, decrypted, skipped=0, fails=0, sent=None, acked=0, timeouts=0):
    return {'t': t, 'position': position, 'decryptedMs': decrypted, 'skippedMs': skipped, 'decryptFails': fails, 'sentMs': decrypted if sent is None else sent,
            'readinessDips': 0, 'outputUnderflows': 0, 'drainsAcked': acked, 'drainTimeouts': timeouts}


def judged():
    return {'base': counts(1000.0, None, 0), 'heard': counts(1001.0, 0.4, 900), 'end': counts(1038.0, 37.4, 38000), 'itemId': SONG, 'state': 'Playing', 'banner': None, 'duration': 355.2}


def played(n=5, **extra):
    return dict({'t0': 1000.0, 't1': 1050.0, 'started': True, 'song': SONG, 'reps': [{'audioMs': 800, 'exit': 0, 'error': False, 'banner': None}] * 5,
                 'renderers': [RADEONSI], 'judged': [judged()] * n}, **extra)


def move(duration=185.0, start=0.0, cut=False):
    target, flip = duration - 20, 1000.0 + 20
    samples = [[round(1000.0 + i / 4, 2), start + target + i / 4] for i in range(int(4 * 26))]
    return {'reached': True, 'duration': duration, 'target': target, 'start': start, 'cut': cut, 'samples': samples,
            'base': counts(1000.0, start + target + 0.2, 0, sent=0), 'flip': counts(flip, start + duration, 20000, sent=20000), 'end': counts(flip + 5.1, start + duration + 5, 25000, sent=25000)}


def f5_rounds(**cmd):
    row = {c: dict({'exit': 0, 'appliedMs': 30, 'effectMs': 40}, **({'advancingMs': 400} if c in ('next', 'prev') else {})) for c in gate.F5_COMMANDS}
    return [dict(row, **cmd)] * 5


def write_valid(out, renderer=RADEONSI, mode='counted'):
    w = lambda name, data: gate.save(out, name, data)
    w('build.json', {'commit': 'c0ffee', 'dirty': False, 'mode': mode, 'shippedFeaturesExit': 0, 'shippedBytes': 22834176, 'ballastFeaturesExit': 1,
                     'ballastFeaturesOutput': 'FAIL: ships test-only features:\n  app/gate-sabotage'})
    w('session.verdict.json', {'sink': SINK, 'defaultAtStart': 'iec958', 'defaultAtEnd': 'iec958', 'sinkGone': True})
    weston_says(f'GL renderer: {RADEONSI}')(out)
    for ref in gate.REFERENCE_RUNS:
        w(f'{ref}.json', {'sha256': gate.REFERENCE['sha256'], 'pid': 7, 'samples': rows(20, round(gate.REFERENCE['pss'] + 0.7, 1), 62.0, 104.0, playing=False)})
    log = [[990.0 + i / 4, [SINK]] for i in range(400)]
    for name in SESSIONS:
        w(f'{name}.guard.json', {'pid': 7, 'killed': None, 'log': log, 'modes': OFF})
    for name, r1 in (('shipped', (170.0, 148.0, 198.0)), ('ballast', (233.0, 215.0, 260.0))):
        w(f'{name}.r2.json', {'pid': 7, 'samples': rows(20, 72.0, 50.0, 98.0, renderer, playing=False)})
        w(f'{name}.r1.json', {'pid': 7, 'started': True, 'prePlay': {'default': 'iec958'}, 'ballastMiB': 67 if name == 'ballast' else None, 'samples': rows(60, *r1, renderer)})
    put('ballast.r1.json', baseSamples=rows(60, 167.0, 148.0, 195.0, renderer), ballastTouched=67)(out)
    s = lambda name, **ev: dict(ev, session=name, renderers=[renderer])
    w('f1.json', {'starts': [{'ms': 110, 'route': 'Home', 'playlists': 34}] * 5, 'renderers': [renderer] * 5, 'sessions': ['shipped']})
    w('f2.json', s('signedout', t0=1000.0, t1=1020.0, banner='SignInNeeded', playExit=5, played=False, streams=[]))
    w('f3.json', s('browse', t0=1000.0, t1=1010.0, reps=[{'exit': 0, 'wallMs': 250, 'songs': 10, 'uiMs': 420}] * 5))
    w('f4.json', dict(played(), sessions=['shipped'], renderers=[renderer]))
    w('f5.json', s('shipped', t0=1000.0, t1=1050.0, started=True, rounds=f5_rounds()))
    w('f6.json', s('shipped', t0=1000.0, t1=1050.0, started=True, transitions=[move(), move(173.0, 185.0), move(211.0, 358.0)]))
    w('f7.json', {'mcpTotal': 34, 'largestTracks': 250, 'viewTotal': 250, 'endReached': True, 'artTextures': 60, 'renderers': [renderer]})
    w('f8.json', s('shipped', t0=1000.0, t1=1050.0, started=True, expected=['a', 'b', 'c', 'd'], seen=['a', 'b', 'c', 'd'], exits=[0] * 4))
    w('f9.json', s('browse', **played(1, tools=gate.TOOLS, searchHasSong=True)))
    w('f10-badid.json', s('browse', t0=1000.0, t1=1010.0, alive=True, bannerBefore=None, banner='Error("setQueue: NOT_FOUND")', refused=None))
    w('f10-net.json', s('offline', t0=1000.0, t1=1010.0, downError=True, downUi='error', alive=True, coversRecoveredS=4.0, searchAfter=True, uiAfter='results',
                        probeCold=False, probeUp=True, probeDropped=False, probeBack=True, midDrop={'exit': 1, 'ui': 'error'}, afterDrop={'exit': 0, 'ui': 'results'}, aliveAtEnd=True,
                        dropGapS=4.2, holeRecoveredS=11.0))
    w('r3.json', s('shipped', t0=1000.0, t1=2810.0, started=True, pid=7, samples=soak(), seekTimes=[]))
    cpu = lambda state, load: dict(t0=1000.0, t1=1060.0, before=state, after=state, cpu=[load] * 30, mean=load, binS=10, procs=[1] * 30)
    w('r4.json', s('cpu', started=True, **cpu('Playing', 4.0)))
    w('r5.json', s('cpu', **cpu('Paused', 0.5)))
    w('errors.json', {})
    real = {'tokenMtimeNs': 1, 'tokenSize': 300, 'keyringItems': [], 'cache': None, 'config': {'developer-token': [1, 2]}}
    w('realstate.json', {'before': real, 'after': real})
    w('cotenants.json', {'polls': 600, 'seen': []})
    w('pacing.json', {'limit': 20, 'windowS': 600, 'prior': [400.0], 'actions': [[1000.0 + 30 * i, 'play'] for i in range(20)], 'waits': [[1300.0, 120.5, 20]]})
    w('leakgrep.json', {'libraryNames': 34, 'catalogNames': 40, 'tokenFragments': 6, 'runFiles': 90, 'hits': {'repo': 0, 'run': 0}, 'plantedRun': True, 'plantedRepo': True})


def edit(name, fn):
    def apply(out):
        data = gate.load(out, name)
        fn(data)
        gate.save(out, name, data)
    return apply


def put(name, **values):
    return edit(name, lambda d: d.update(values))


def every(name, key, value, kind='r1'):
    return edit(f'{name}.{kind}.json', lambda d: [r.__setitem__(key, value) for r in d['samples']])


def remove(name):
    return lambda out: os.remove(os.path.join(out, name))


def both(*fns):
    return lambda out: [fn(out) for fn in fns]


def drift(by, ref='reference-after'):
    return edit(f'{ref}.json', lambda d: [r.__setitem__('pss', round(gate.REFERENCE['pss'] + by, 1)) for r in d['samples']])


def soak_edit(fn):
    return edit('r3.json', lambda d: [fn(r) for r in d['samples']])


def landing_at(position):
    return both(put('r3.json', seekTimes=[1120.0, 1120.0]), edit('r3.json', lambda d: d['samples'][14].__setitem__('position', position)))


def grow(key, by):
    return soak_edit(lambda r: r.__setitem__(key, r[key] + (by if r['m'] >= 29 else 0)))


ZONE3 = both(every('shipped', 'rss', 205.0), every('shipped', 'pss', 180.0))
ZONE3_OVER = both(ZONE3, every('shipped', 'pss', 201.0))
F5_SLOW = lambda cmd, ms, reps=5: edit('f5.json', lambda d: d.__setitem__('rounds', [dict(r, **{cmd: {'matched': True, 'ms': ms if i < reps else 40}}) for i, r in enumerate(d['rounds'])]))
MEMORY = [
    ('R2 engine started', edit('shipped.r2.json', lambda d: d['samples'][3].__setitem__('engineStarted', True)), 'R2', 'FAIL', 'engine started'),
    ('R2 USS over the bar', every('shipped', 'uss', 101.0, 'r2'), 'R2', 'FAIL', 'USS median 101.0'),
    ('fewer than 50 covers', edit('shipped.r1.json', lambda d: d['samples'][5].__setitem__('texturesUploaded', 49)), 'R1', 'FAIL', 'covers'),
    ('two processes', edit('shipped.r1.json', lambda d: d['samples'][5].__setitem__('procs', 2)), 'R1', 'FAIL', 'exactly 1 process'),
    ('too few samples', edit('shipped.r1.json', lambda d: d['samples'].pop()), 'R1', 'FAIL', '59 samples'),
    ('never started', put('shipped.r1.json', started=False), 'R1', 'FAIL', 'never started'),
    ('position stuck', every('shipped', 'position', 20.0), 'R1', 'FAIL', 'advanced 0.0'),
    ('paused in a sample', edit('shipped.r1.json', lambda d: d['samples'][9].__setitem__('state', 'Paused')), 'R1', 'FAIL', 'not Playing'),
    ('banner in a sample', edit('shipped.r1.json', lambda d: d['samples'][9].__setitem__('banner', 'SignInNeeded')), 'R1', 'FAIL', 'not Playing'),
    ('guard kill voids R1', put('shipped.guard.json', killed='linked to spdif'), 'R1', 'VOID', 'guard killed'),
    ('guard kill voids R2', put('shipped.guard.json', killed='linked to spdif'), 'R2', 'VOID', 'guard killed'),
    ('no guard polls in the window', put('shipped.guard.json', log=[]), 'R1', 'VOID', 'polls in the R1 window <'),
    ('polls only outside the window', put('shipped.guard.json', log=[[900.0 + i / 4, [SINK]] for i in range(200)] + [[1100.0 + i / 4, [SINK]] for i in range(200)]), 'R1', 'VOID', 'polls in the R1 window <'),
    ('link drops mid-window', edit('shipped.guard.json', lambda d: d['log'][100].__setitem__(1, [])), 'R1', 'VOID', 'not linked'),
    ('a stream with an extra target', edit('shipped.guard.json', lambda d: d['log'][100].__setitem__(1, [SINK, 'hdmi'])), 'R1', 'VOID', 'not linked'),
    ('reference sha', put('reference.json', sha256='feed'), 'R1', 'VOID', 'sha'),
    ('reference renderer', edit('reference-after.json', lambda d: d['samples'][0].__setitem__('glRenderer', LLVMPIPE)), 'R2', 'VOID', 'reference-after renderer'),
    ('reference pid', edit('reference.json', lambda d: d['samples'][0].__setitem__('pid', 8)), 'R2', 'VOID', 'reference snapshot'),
    ('zone 3 drift after', both(ZONE3, drift(-7.3)), 'R1', 'VOID', 'drifted -7.3'),
    ('zone 2 drift reported only', drift(-7.3), 'R1', 'PASS', None),
    ('upward drift lets a PASS stand', both(ZONE3, drift(+6.0)), 'R1', 'PASS', None),
    ('upward drift voids a FAIL', both(ZONE3_OVER, drift(+6.0)), 'R1', 'VOID', 'drifted +6.0 MiB'),
    ('downward drift voids a PASS', both(ZONE3, drift(-6.0)), 'R1', 'VOID', 'drifted -6.0 MiB'),
    ('downward drift lets a FAIL stand', both(ZONE3_OVER, drift(-6.0)), 'R1', 'FAIL', 'PSS median 201.0'),
    ('upward drift at the bar lets a FAIL stand', both(ZONE3_OVER, drift(+5.0)), 'R1', 'FAIL', 'PSS median 201.0'),
    ('upward drift just over the bar voids a FAIL', both(ZONE3_OVER, drift(+5.1)), 'R1', 'VOID', 'drifted +5.1 MiB'),
    ('downward drift at the bar lets a PASS stand', both(ZONE3, drift(-5.0)), 'R1', 'PASS', None),
    ('downward drift just over the bar voids a PASS', both(ZONE3, drift(-5.1)), 'R1', 'VOID', 'drifted -5.1 MiB'),
    ('upward drift before the app voids a FAIL', both(ZONE3_OVER, drift(+6.0, 'reference')), 'R1', 'VOID', 'drifted +6.0 MiB'),
    ('readings drifting both ways void a PASS', both(ZONE3, drift(+6.0, 'reference'), drift(-6.0)), 'R1', 'VOID', 'drifted -6.0 MiB'),
    ('readings drifting both ways void a FAIL', both(ZONE3_OVER, drift(-6.0, 'reference'), drift(+6.0)), 'R1', 'VOID', 'drifted +6.0 MiB'),
    ('zone 3 PSS over', ZONE3_OVER, 'R1', 'FAIL', 'PSS median 201.0'),
    ('default sink changed', put('session.verdict.json', defaultAtEnd='hdmi'), 'R1', 'VOID', 'default sink'),
    ('wpctl failed', put('session.verdict.json', defaultAtStart=None, defaultAtEnd=None), 'R2', 'VOID', 'default sink'),
    ('default restored', put('session.verdict.json', defaultRestored=True), 'R1', 'VOID', 'default sink'),
    ('wpctl failed before play', edit('shipped.r1.json', lambda d: d['prePlay'].__setitem__('default', None)), 'R1', 'VOID', 'unreadable before play'),
    ('null sink left', put('session.verdict.json', sinkGone=False), 'R1', 'VOID', 'left behind'),
    ('weston on llvmpipe', weston_says(f'GL renderer: {LLVMPIPE}'), 'R2', 'VOID', 'weston renderer'),
    ('weston names llvmpipe beside radeonsi', weston_says(f'GL renderer: {RADEONSI} via llvmpipe'), 'R2', 'VOID', 'weston renderer'),
    ('weston log without a renderer line', weston_says('weston 14.0.2'), 'R1', 'VOID', 'weston renderer'),
    ('app renderer not radeonsi', every('shipped', 'glRenderer', 'Some GPU (nouveau)'), 'R1', 'VOID', 'shipped renderer'),
    ('app renderer llvmpipe beside radeonsi', every('shipped', 'glRenderer', f'{RADEONSI} {LLVMPIPE}'), 'R1', 'VOID', 'shipped renderer'),
    ('shipped features', put('build.json', shippedFeaturesExit=1), 'R2', 'FAIL', 'check-shipped-features'),
    ('dirty tree', put('build.json', dirty=True), 'R1', 'FAIL', 'dirty'),
    ('dirty unknown', edit('build.json', lambda d: d.pop('dirty')), 'R2', 'FAIL', 'dirty'),
    ('snapshot from another pid', edit('shipped.r1.json', lambda d: d['samples'][4].__setitem__('pid', 8)), 'R1', 'FAIL', 'another process'),
    ('stored medians ignored', both(put('shipped.r1.json', pss=1.0, uss=1.0, rss=1.0), every('shipped', 'uss', 250.0)), 'R1', 'FAIL', 'USS median 250.0'),
    ('missing build beats void', both(remove('build.json'), put('session.verdict.json', sinkGone=False)), 'R1', 'FAIL', 'build.json'),
    ('missing session', remove('session.verdict.json'), 'R2', 'FAIL', 'session.verdict.json'),
    ('missing reference after', remove('reference-after.json'), 'R1', 'FAIL', 'reference-after.json'),
    ('missing weston log', remove('weston.log'), 'R1', 'FAIL', 'weston.log'),
    ('a co-tenant during the run', put('cotenants.json', seen=[[1500.0, ['123 weston []']]]), 'R1', 'VOID', 'another weston'),
    ('co-tenant watch never polled', put('cotenants.json', polls=0), 'R2', 'FAIL', 'cotenants.json'),
    ('co-tenant record missing', remove('cotenants.json'), 'R1', 'FAIL', 'cotenants.json'),
    ('missing guard', remove('shipped.guard.json'), 'R2', 'FAIL', 'shipped.guard.json'),
    ('missing r1 in a void session', both(remove('shipped.r1.json'), put('session.verdict.json', sinkGone=False)), 'R1', 'FAIL', 'shipped.r1.json'),
]
FUNCTIONAL = [
    ('F1 one slow start fails', put('f1.json', starts=[{'ms': 501, 'route': 'Home', 'playlists': 3}] + [{'ms': 100, 'route': 'Home', 'playlists': 3}] * 4), 'F1', 'FAIL', 'slowest first frame 501'),
    ('F1 every start at the bar', put('f1.json', starts=[{'ms': 500, 'route': 'Home', 'playlists': 3}] * 5), 'F1', 'PASS', None),
    ('F1 no frame', put('f1.json', starts=[{'ms': None, 'route': 'Home', 'playlists': 3}] + [{'ms': 100, 'route': 'Home', 'playlists': 3}] * 4), 'F1', 'FAIL', '4 of 5 starts'),
    ('F1 four starts', put('f1.json', starts=[{'ms': 100, 'route': 'Home', 'playlists': 3}] * 4), 'F1', 'FAIL', '4 of 5 starts'),
    ('F1 no sidebar', put('f1.json', starts=[{'ms': 100, 'route': 'Home', 'playlists': 0}] * 5), 'F1', 'FAIL', 'sidebar'),
    ('F1 not Home', put('f1.json', starts=[{'ms': 100, 'route': 'Search', 'playlists': 3}] * 5), 'F1', 'FAIL', 'sidebar'),
    ('F1 llvmpipe', put('f1.json', renderers=[LLVMPIPE] * 5), 'F1', 'VOID', 'renderer'),
    ('F2 no banner', put('f2.json', banner=None), 'F2', 'FAIL', 'banner'),
    ('F2 play accepted', put('f2.json', playExit=0), 'F2', 'FAIL', 'not refused'),
    ('F2 played', put('f2.json', played=True), 'F2', 'FAIL', 'not refused'),
    ('F2 a stream appeared', put('f2.json', streams=[SINK]), 'F2', 'FAIL', 'not refused'),
    ('F2 guard kill', put('signedout.guard.json', killed='linked to spdif'), 'F2', 'VOID', 'guard killed'),
    *[(f'{c} session started shuffled', put(f'{s}.guard.json', modes=dict(OFF, shuffleMode='on')), c, 'VOID', 'shuffle and repeat off')
      for c, s in (('F1', 'shipped'), ('F2', 'signedout'), ('F3', 'browse'), ('F4', 'shipped'), ('F5', 'shipped'), ('F6', 'shipped'), ('F8', 'shipped'), ('F9', 'browse'), ('R3', 'shipped'), ('R4', 'cpu'))],
    ('F4 session started on repeat one', put('shipped.guard.json', modes=dict(OFF, repeatMode='one')), 'F4', 'VOID', 'shuffle and repeat off'),
    ('F4 modes never read', put('shipped.guard.json', modes={'shuffleMode': None, 'repeatMode': None}), 'F4', 'FAIL', 'modes in shipped.guard.json'),
    ('F4 one mode never read', put('shipped.guard.json', modes={'shuffleMode': 'off', 'repeatMode': None}), 'F4', 'FAIL', 'modes in shipped.guard.json'),
    ('F4 modes recorded as null', put('shipped.guard.json', modes=None), 'F4', 'FAIL', 'modes in shipped.guard.json'),
    ('F4 modes not recorded', edit('shipped.guard.json', lambda d: d.pop('modes')), 'F4', 'FAIL', 'modes in shipped.guard.json'),
    ('R1 session started shuffled', put('shipped.guard.json', modes=dict(OFF, shuffleMode='on')), 'R1', 'VOID', 'shuffle and repeat off'),
    ('R2 session started on repeat all', put('shipped.guard.json', modes=dict(OFF, repeatMode='all')), 'R2', 'VOID', 'shuffle and repeat off'),
    ('R1 modes not recorded', edit('shipped.guard.json', lambda d: d.pop('modes')), 'R1', 'FAIL', 'modes in shipped.guard.json'),
    ('F3 slow CLI', put('f3.json', reps=[{'exit': 0, 'wallMs': 2100, 'songs': 3, 'uiMs': 2100}] * 5), 'F3', 'FAIL', 'wallMs median 2100'),
    ('F3 slow view', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 2100}] * 5), 'F3', 'FAIL', 'uiMs median 2100'),
    ('F3 view never showed', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': None}] + [{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 4), 'F3', 'FAIL', 'Search view'),
    ('F3 no songs', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 0, 'uiMs': 400}] * 5), 'F3', 'FAIL', 'did not return songs'),
    ('F3 error exit', put('f3.json', reps=[{'exit': 1, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 5), 'F3', 'FAIL', 'did not return songs'),
    ('F3 four reps', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 4), 'F3', 'FAIL', 'did not return songs'),
    ('F4 never started', put('f4.json', started=False, reps=[{'audioMs': None}] * 5), 'F4', 'FAIL', 'never started'),
    ('F4 slow audio', put('f4.json', reps=[{'audioMs': 3100, 'exit': 0}] * 5), 'F4', 'FAIL', 'audio median 3100'),
    ('F4 slow median beside a fast play', put('f4.json', reps=[{'audioMs': 800, 'exit': 0}] * 2 + [{'audioMs': 3100, 'exit': 0}] * 3), 'F4', 'FAIL', 'audio median 3100'),
    ('F4 fast median beside slow plays', put('f4.json', reps=[{'audioMs': 800, 'exit': 0}] * 3 + [{'audioMs': 9000, 'exit': 0}] * 2), 'F4', 'PASS', None),
    ('F4 one missed play', put('f4.json', reps=[{'audioMs': None, 'exit': 0}] + [{'audioMs': 800, 'exit': 0}] * 4), 'F4', 'FAIL', '4 of 5'),
    ('F4 a play exits non-zero', put('f4.json', reps=[{'audioMs': 800, 'exit': 1}] + [{'audioMs': 800, 'exit': 0}] * 4), 'F4', 'FAIL', 'did not exit 0'),
    ('F4 four judged plays', edit('f4.json', lambda d: d['judged'].pop()), 'F4', 'FAIL', '4 judged plays < 5'),
    ('F4 decrypted short', edit('f4.json', lambda d: d['judged'][2]['end'].__setitem__('decryptedMs', 34999)), 'F4', 'FAIL', 'decrypted 34999 ms < 35000'),
    ('F4 decrypted at the bar', edit('f4.json', lambda d: d['judged'][2]['end'].__setitem__('decryptedMs', 35000)), 'F4', 'PASS', None),
    ('F4 decrypted counted before the play', edit('f4.json', lambda d: d['judged'][2]['base'].__setitem__('decryptedMs', 10000)), 'F4', 'FAIL', 'decrypted 28000 ms'),
    ('F4 skipped over', edit('f4.json', lambda d: d['judged'][0]['end'].__setitem__('skippedMs', 101)), 'F4', 'FAIL', 'skipped 101 ms > 100'),
    ('F4 skipped at the bar', edit('f4.json', lambda d: d['judged'][0]['end'].__setitem__('skippedMs', 100)), 'F4', 'PASS', None),
    ('F9 a previous play flushed before this one was heard', edit('f9.json', lambda d: [d['judged'][0]['heard'].__setitem__('skippedMs', 1513), d['judged'][0]['end'].__setitem__('skippedMs', 1513)]), 'F9', 'PASS', None),
    ('F9 skipped while audible', edit('f9.json', lambda d: [d['judged'][0]['heard'].__setitem__('skippedMs', 1513), d['judged'][0]['end'].__setitem__('skippedMs', 1614)]), 'F9', 'FAIL', 'skipped 101 ms'),
    ('F4 a decrypt failure', edit('f4.json', lambda d: d['judged'][0]['end'].__setitem__('decryptFails', 1)), 'F4', 'FAIL', 'decrypt failures 1'),
    ('F4 decrypt failures unreported', edit('f4.json', lambda d: [j[k].pop('decryptFails') for j in d['judged'] for k in ('base', 'heard', 'end')]), 'F4', 'FAIL', "counters missing: ['decryptFails']"),
    ('F4 skipped unreported', edit('f4.json', lambda d: [j[k].pop('skippedMs') for j in d['judged'] for k in ('base', 'heard', 'end')]), 'F4', 'FAIL', "counters missing: ['skippedMs']"),
    ('F4 skipped unreported at the end only', edit('f4.json', lambda d: d['judged'][3]['end'].__setitem__('skippedMs', None)), 'F4', 'FAIL', "counters missing: ['skippedMs']"),
    ('F9 counters unreported', edit('f9.json', lambda d: [d['judged'][0][k].pop(c) for k in ('base', 'heard', 'end') for c in ('decryptFails', 'skippedMs')]), 'F9', 'FAIL', "counters missing: ['decryptFails', 'skippedMs']"),
    ('F6 counters unreported', edit('f6.json', lambda d: d['transitions'][1]['end'].pop('skippedMs')), 'F6', 'FAIL', '2 of 3'),
    ('F6 drain counters unreported', edit('f6.json', lambda d: [d['transitions'][0][k].pop('drainTimeouts') for k in ('base', 'flip', 'end')]), 'F6', 'FAIL', '2 of 3'),
    ('F4 position behind the wall clock', edit('f4.json', lambda d: d['judged'][1]['end'].__setitem__('t', 1039.1)), 'F4', 'FAIL', 'wall clock'),
    ('F4 position ahead of the wall clock', edit('f4.json', lambda d: d['judged'][1]['end'].__setitem__('t', 1035.9)), 'F4', 'FAIL', 'wall clock'),
    ('F4 pace at the tolerance', edit('f4.json', lambda d: d['judged'][1]['end'].__setitem__('t', 1038.0)), 'F4', 'PASS', None),
    ('F4 banner at the end', edit('f4.json', lambda d: d['judged'][4].__setitem__('banner', 'Error')), 'F4', 'FAIL', 'no banner'),
    ('F4 paused at the end', edit('f4.json', lambda d: d['judged'][4].__setitem__('state', 'Paused')), 'F4', 'FAIL', 'no banner'),
    ('F4 another song', edit('f4.json', lambda d: d['judged'][4].__setitem__('itemId', '1')), 'F4', 'FAIL', 'no banner'),
    ('F4 stopped at 35', edit('f4.json', lambda d: d['judged'][4]['end'].__setitem__('position', 35.0)), 'F4', 'FAIL', 'no banner'),
    ('F4 preview', edit('f4.json', lambda d: d['judged'][4].__setitem__('duration', 30.0)), 'F4', 'FAIL', 'duration 30.0'),
    ('F4 never started', put('f4.json', started=False, reps=[{'audioMs': None, 'exit': 0}] * 5, judged=[]), 'F4', 'FAIL', 'never started'),
    ('F4 no stream once started', put('shipped.guard.json', log=[[990.0 + i / 4, []] for i in range(400)]), 'F4', 'VOID', 'no stream'),
    ('F4 stream elsewhere once started', edit('shipped.guard.json', lambda d: d['log'][60].__setitem__(1, ['hdmi'])), 'F4', 'VOID', 'no stream'),
    ('F4 stream outside its window only', put('shipped.guard.json', log=[[900.0, [SINK]], [1100.0, [SINK]]]), 'F4', 'VOID', 'no stream'),
    ('F4 never started and no stream is FAIL', both(put('f4.json', started=False), put('shipped.guard.json', log=[])), 'F4', 'FAIL', 'never started'),
    ('F4 a cold launch guard is missing', put('f4.json', sessions=['shipped', 'f4cold-gone']), 'F4', 'FAIL', 'f4cold-gone.guard.json'),
    ('F4 a cold launch guard killed', both(put('f4.json', sessions=['cpu', 'shipped']), put('cpu.guard.json', killed='linked to spdif')), 'F4', 'VOID', 'guard killed'),
    ('F4 missing guard', remove('shipped.guard.json'), 'F4', 'FAIL', 'shipped.guard.json'),
    ('F4 renderer', put('f4.json', renderers=['Some GPU (nouveau)']), 'F4', 'VOID', 'renderer'),
    ('F5 slow seek median', put('f5.json', rounds=f5_rounds()[:2] + f5_rounds(seek={'exit': 0, 'appliedMs': 30, 'effectMs': 530})[:3]), 'F5', 'FAIL', 'seek: median 530'),
    ('F5 one slow seek is fine', put('f5.json', rounds=f5_rounds()[:4] + f5_rounds(seek={'exit': 0, 'appliedMs': 30, 'effectMs': 900})[:1]), 'F5', 'PASS', None),
    ('F5 one slow pause fails', put('f5.json', rounds=f5_rounds()[:4] + f5_rounds(pause={'exit': 0, 'appliedMs': 30, 'effectMs': 501})[:1]), 'F5', 'FAIL', 'pause: slowest 501'),
    ('F5 one slow resume fails', put('f5.json', rounds=f5_rounds()[:4] + f5_rounds(resume={'exit': 0, 'appliedMs': 30, 'effectMs': 501})[:1]), 'F5', 'FAIL', 'resume: slowest 501'),
    ('F5 resume at the bar', put('f5.json', rounds=f5_rounds(resume={'exit': 0, 'appliedMs': 30, 'effectMs': 500})), 'F5', 'PASS', None),
    ('F5 acknowledged but no effect', put('f5.json', rounds=f5_rounds(seek={'exit': 0, 'appliedMs': 30, 'effectMs': None})), 'F5', 'FAIL', 'seek: a command failed'),
    ('F5 effect without the acknowledgement', put('f5.json', rounds=f5_rounds(pause={'exit': 0, 'appliedMs': None, 'effectMs': 40})), 'F5', 'FAIL', 'pause: a command failed'),
    ('F5 a command exits non-zero', put('f5.json', rounds=f5_rounds(prev={'exit': 1, 'appliedMs': 30, 'effectMs': 40, 'advancingMs': 400})), 'F5', 'FAIL', 'prev: a command failed'),
    ('F5 next never advances', put('f5.json', rounds=f5_rounds(next={'exit': 0, 'appliedMs': 30, 'effectMs': 40, 'advancingMs': None})), 'F5', 'FAIL', 'next: the new item never advanced'),
    ('F5 prev advances late', put('f5.json', rounds=f5_rounds(prev={'exit': 0, 'appliedMs': 30, 'effectMs': 40, 'advancingMs': 3001})), 'F5', 'FAIL', 'prev: new item advancing median 3001'),
    ('F5 next advances at the bar', put('f5.json', rounds=f5_rounds(next={'exit': 0, 'appliedMs': 30, 'effectMs': 40, 'advancingMs': 3000})), 'F5', 'PASS', None),
    ('F5 four rounds', edit('f5.json', lambda d: d['rounds'].pop()), 'F5', 'FAIL', '4 rounds'),
    ('F6 skipped over', edit('f6.json', lambda d: d['transitions'][1]['end'].__setitem__('skippedMs', 101)), 'F6', 'FAIL', '2 of 3'),
    ('F6 skipped at the bar', edit('f6.json', lambda d: d['transitions'][1]['end'].__setitem__('skippedMs', 100)), 'F6', 'PASS', None),
    ('F6 skipped after the flip counts', edit('f6.json', lambda d: d['transitions'][1]['end'].__setitem__('skippedMs', 3000)), 'F6', 'FAIL', '2 of 3'),
    ('F6 early contiguous cut: sent short at the flip', edit('f6.json', lambda d: d['transitions'][0]['flip'].__setitem__('sentMs', 16999)), 'F6', 'FAIL', '2 of 3'),
    ('F6 sent at the slack', edit('f6.json', lambda d: d['transitions'][0]['flip'].__setitem__('sentMs', 17000)), 'F6', 'PASS', None),
    ('F6 sent read at the flip, not the window end', both(edit('f6.json', lambda d: d['transitions'][0]['flip'].__setitem__('sentMs', 12000)),
                                                         edit('f6.json', lambda d: d['transitions'][0]['end'].__setitem__('sentMs', 30000))), 'F6', 'FAIL', '2 of 3'),
    ('F6 stall over a second', edit('f6.json', lambda d: [p.__setitem__(1, d['transitions'][2]['start'] + 191.0) for p in d['transitions'][2]['samples'] if 1019.0 <= p[0] <= 1020.1]), 'F6', 'FAIL', '2 of 3'),
    ('F6 mislanded', edit('f6.json', lambda d: d['transitions'][0]['base'].__setitem__('position', 166.3)), 'F6', 'FAIL', '2 of 3'),
    ('F6 landed at the bar', edit('f6.json', lambda d: d['transitions'][0]['base'].__setitem__('position', 166.0)), 'F6', 'PASS', None),
    ('F6 drained rescues a skip', both(edit('f6.json', lambda d: d['transitions'][1]['end'].__setitem__('skippedMs', 500)), edit('f6.json', lambda d: d['transitions'][1]['end'].__setitem__('drainsAcked', 1))), 'F6', 'PASS', None),
    ('F6 a drain timeout in the window', both(edit('f6.json', lambda d: d['transitions'][1]['end'].update(skippedMs=500, drainsAcked=1, drainTimeouts=1))), 'F6', 'FAIL', '2 of 3'),
    ('F6 a drain acked only at the flip is read at the window end', both(edit('f6.json', lambda d: d['transitions'][1]['flip'].__setitem__('drainsAcked', 1)), edit('f6.json', lambda d: d['transitions'][1]['end'].update(skippedMs=500, drainTimeouts=1, drainsAcked=1))), 'F6', 'FAIL', '2 of 3'),
    ('F6 window shorter than 5 s', edit('f6.json', lambda d: d['transitions'][2]['end'].__setitem__('t', 1024.9)), 'F6', 'FAIL', '2 of 3'),
    ('F6 no flip', edit('f6.json', lambda d: d['transitions'][2].__setitem__('flip', None)), 'F6', 'FAIL', '2 of 3'),
    ('F6 two transitions', edit('f6.json', lambda d: d['transitions'].pop()), 'F6', 'FAIL', '2 of 3'),
    ('F7 no playlists', put('f7.json', mcpTotal=0), 'F7', 'FAIL', 'listed no playlists'),
    ('F7 short playlist', put('f7.json', largestTracks=99), 'F7', 'FAIL', '99 tracks'),
    ('F7 end not reached', put('f7.json', endReached=False), 'F7', 'FAIL', 'end'),
    ('F7 no covers', put('f7.json', artTextures=0), 'F7', 'FAIL', 'end'),
    ('F7 another playlist', put('f7.json', viewTotal=120), 'F7', 'FAIL', 'not the largest'),
    ('F8 out of order', put('f8.json', seen=['a', 'c', 'b', 'd']), 'F8', 'FAIL', 'played'),
    ('F8 a failed add', put('f8.json', exits=[0, 0, 1, 0]), 'F8', 'FAIL', 'played'),
    ('F8 nothing expected', put('f8.json', expected=[], seen=[]), 'F8', 'FAIL', 'played'),
    ('F9 missing tool', put('f9.json', tools=gate.TOOLS[1:]), 'F9', 'FAIL', 'tools/list'),
    ('F9 search miss', put('f9.json', searchHasSong=False), 'F9', 'FAIL', 'MCP search'),
    ('F9 one MCP play errors', put('f9.json', reps=[{'audioMs': 800, 'exit': 0, 'error': True}] + [{'audioMs': 800, 'exit': 0, 'error': False}] * 4), 'F9', 'FAIL', 'MCP play returned an error'),
    ('F9 error unrecorded', put('f9.json', reps=[{'audioMs': 800}] * 5), 'F9', 'FAIL', 'MCP play returned an error'),
    ('F9 slow audio', put('f9.json', reps=[{'audioMs': 3001}] * 5), 'F9', 'FAIL', 'audio median'),
    ('F9 no stream', put('browse.guard.json', log=[]), 'F9', 'VOID', 'no stream'),
    ('F10 bad id crashed', put('f10-badid.json', alive=False), 'F10', 'FAIL', 'bad id'),
    ('F10 bad id silent', put('f10-badid.json', banner=None), 'F10', 'FAIL', 'bad id'),
    ('F10 banner left from before', put('f10-badid.json', bannerBefore='Error("setQueue: NOT_FOUND")'), 'F10', 'FAIL', 'bad id'),
    ('F10 bad id refused', put('f10-badid.json', banner=None, refused='not a catalog id'), 'F10', 'PASS', None),
    ('F10 drop never happened', put('f10-net.json', probeDropped=True), 'F10', 'FAIL', 'probes'),
    ('F10 cold leg online', put('f10-net.json', probeCold=True), 'F10', 'FAIL', 'probes'),
    ('F10 drop search passed', put('f10-net.json', midDrop={'exit': 0, 'ui': 'results'}), 'F10', 'FAIL', 'mid-session'),
    ('F10 CLI fails after drop', put('f10-net.json', afterDrop={'exit': 1, 'ui': 'results'}), 'F10', 'FAIL', 'mid-session'),
    ('F10 view stuck after drop', put('f10-net.json', afterDrop={'exit': 0, 'ui': 'error'}), 'F10', 'FAIL', 'mid-session'),
    ('F10 dead at end', put('f10-net.json', aliveAtEnd=False), 'F10', 'FAIL', 'alive at the end'),
    ('F10 drop leg too slow', put('f10-net.json', dropGapS=15.0), 'F10', 'VOID', 'aged out'),
    ('F10 drop gap unrecorded', put('f10-net.json', dropGapS=None), 'F10', 'VOID', 'aged out'),
    ('F10 hole never recovered', put('f10-net.json', holeRecoveredS=None), 'F10', 'FAIL', 'black hole'),
    ('F10 hole recovered late', put('f10-net.json', holeRecoveredS=20.5), 'F10', 'FAIL', 'black hole'),
    ('F10 hole at the bar', put('f10-net.json', holeRecoveredS=20.0), 'F10', 'PASS', None),
    ('F10 offline search passed', put('f10-net.json', downError=False), 'F10', 'FAIL', 'network down'),
    ('F10 offline view quiet', put('f10-net.json', downUi='idle'), 'F10', 'FAIL', 'network down'),
    ('F10 offline crash', put('f10-net.json', alive=False), 'F10', 'FAIL', 'network down'),
    ('F10 covers never came back', put('f10-net.json', coversRecoveredS=None), 'F10', 'FAIL', 'recovery'),
    ('F10 search never came back', put('f10-net.json', searchAfter=False), 'F10', 'FAIL', 'recovery'),
    ('F10 view never came back', put('f10-net.json', uiAfter='error'), 'F10', 'FAIL', 'recovery'),
    ('F10 missing net evidence', remove('f10-net.json'), 'F10', 'FAIL', 'f10-net.json'),
    ('F10 offline guard missing', remove('offline.guard.json'), 'F10', 'FAIL', 'offline.guard.json'),
    ('R3 USS growth', grow('uss', 10.1), 'R3', 'FAIL', 'USS growth 10.1'),
    ('R3 PSS growth', grow('pss', 10.1), 'R3', 'FAIL', 'PSS growth 10.1'),
    ('R3 growth at the bar', both(grow('uss', 10.0), grow('pss', 10.0)), 'R3', 'PASS', None),
    ('R3 growth before minute 5 ignored', soak_edit(lambda r: r.__setitem__('uss', 120.0 if r['m'] < 5 else 148.0)), 'R3', 'PASS', None),
    ('R3 ended early', edit('r3.json', lambda d: d.__setitem__('samples', d['samples'][:175])), 'R3', 'FAIL', 'soak ended at minute'),
    ('R3 seven tracks', soak_edit(lambda r: r.__setitem__('itemId', f'i{min(6, r["m"] // 2.5)}')), 'R3', 'FAIL', '7 tracks played'),
    ('R3 crash', edit('r3.json', lambda d: d['samples'][100].__setitem__('pid', 8)), 'R3', 'FAIL', 'crashed'),
    ('R3 forked', edit('r3.json', lambda d: d['samples'][100].__setitem__('procs', 2)), 'R3', 'FAIL', 'crashed'),
    ('R3 positions carried across items', soak_edit(lambda r: r.__setitem__('position', r['position'] + 160.0 * int(r['itemId'][1:]))), 'R3', 'FAIL', 'position beyond duration + 1.0 s in'),
    ('R3 position at duration + 1', edit('r3.json', lambda d: d['samples'][5].__setitem__('duration', 69.0)), 'R3', 'PASS', None),
    ('R3 position past duration + 1', edit('r3.json', lambda d: d['samples'][5].__setitem__('duration', 68.9)), 'R3', 'FAIL', 'worst +1.1 s on item i0'),
    ('R3 a Playing sample without a duration', edit('r3.json', lambda d: d['samples'][5].pop('duration')), 'R3', 'FAIL', '1 Playing samples without a position or duration'),
    ('R3 a Playing sample without a position', edit('r3.json', lambda d: d['samples'][5].__setitem__('position', None)), 'R3', 'FAIL', '1 Playing samples without a position or duration'),
    ('R3 a seam sample is judged on the old duration and is not a stall', both(edit('r3.json', lambda d: d['samples'][14].__setitem__('duration', 171.0)), edit('r3.json', lambda d: d['samples'][15].update(position=170.4, duration=140.0))), 'R3', 'PASS', None),
    ('R3 a first sample off the old clock is judged on its own duration', both(edit('r3.json', lambda d: d['samples'][14].__setitem__('duration', 171.0)), edit('r3.json', lambda d: d['samples'][15].update(position=160.4, duration=140.0))), 'R3', 'FAIL', 'worst +20.4 s on item i1'),
    ('R3 a seam reset is not a stall', edit('r3.json', lambda d: d['samples'][14].__setitem__('position', 0.2)), 'R3', 'PASS', None),
    ('R3 a seam reset up to the end of the item', both(edit('r3.json', lambda d: d['samples'][14].__setitem__('position', 0.2)), edit('r3.json', lambda d: d['samples'][13].__setitem__('duration', 161.0))), 'R3', 'PASS', None),
    ('R3 a drop short of the end of the item is a stall', both(edit('r3.json', lambda d: d['samples'][14].__setitem__('position', 0.2)), edit('r3.json', lambda d: d['samples'][13].__setitem__('duration', 161.1))), 'R3', 'FAIL', 'behind the wall clock'),
    ('R3 lagging at the end of an item is still a stall', edit('r3.json', lambda d: d['samples'][14].__setitem__('position', 158.9)), 'R3', 'FAIL', 'behind the wall clock'),
    ('R3 a drop in mid-item is a stall', edit('r3.json', lambda d: d['samples'][5].__setitem__('position', 0.2)), 'R3', 'FAIL', 'behind the wall clock'),
    ('R3 stall', soak_edit(lambda r: r.__setitem__('position', r['position'] - (1.1 if r['m'] >= 11 else 0))), 'R3', 'FAIL', 'behind the wall clock'),
    ('R3 lag at the tolerance', soak_edit(lambda r: r.__setitem__('position', r['position'] - (1.0 if r['m'] >= 11 else 0))), 'R3', 'PASS', None),
    ('R3 a seek is not a stall', both(soak_edit(lambda r: r.__setitem__('position', r['position'] - (3.0 if r['m'] >= 12.25 else 0))), put('r3.json', seekTimes=[1735.0])), 'R3', 'PASS', None),
    ('R3 a seek landing at the bar passes', landing_at(155.0), 'R3', 'PASS', None),
    ('R3 a seek landing over the bar fails', landing_at(154.99), 'R3', 'FAIL', 'seek landing: 1 of 1 seeks took over 15.0 s or never landed: [15.01]'),
    ('R3 a seek that never lands before its item changes fails', put('r3.json', seekTimes=[1745.0]), 'R3', 'FAIL', 'seek landing: 1 of 1 seeks took over 15.0 s or never landed: [None]'),
    ('R3 a seek near a pair only excuses that pair', both(soak_edit(lambda r: r.__setitem__('position', r['position'] - (3.0 if r['m'] >= 11 else 0))), put('r3.json', seekTimes=[1000.0 + 10 * 30])), 'R3', 'FAIL', 'behind the wall clock'),
    ('R3 an output underflow', edit('r3.json', lambda d: d['samples'][-1].__setitem__('outputUnderflows', 5)), 'R3', 'FAIL', 'output underflows during the soak: 1'),
    ('R3 output underflows unreadable', soak_edit(lambda r: r.pop('outputUnderflows')), 'R3', 'FAIL', 'outputUnderflows unreadable'),
    ('R3 output underflows unreadable at the first sample', edit('r3.json', lambda d: d['samples'][0].pop('outputUnderflows')), 'R3', 'FAIL', 'outputUnderflows unreadable'),
    ('R3 a readiness dip', edit('r3.json', lambda d: d['samples'][-1].__setitem__('readinessDips', 3)), 'R3', 'FAIL', 'readiness dips during the soak: 1'),
    ('R3 readiness dips unreadable', edit('r3.json', lambda d: d['samples'][0].pop('readinessDips')), 'R3', 'FAIL', 'readinessDips unreadable'),
    ('R3 banner', edit('r3.json', lambda d: d['samples'][50].__setitem__('banner', 'Error')), 'R3', 'FAIL', 'not Playing'),
    ('R3 a track change is not a stall', soak_edit(lambda r: r.__setitem__('position', r['position'] - (3.0 if r['m'] >= 10 else 0) if r['itemId'] != 'i4' or r['m'] < 10 else r['position'])), 'R3', 'PASS', None),
    ('R3 paused', edit('r3.json', lambda d: d['samples'][50].__setitem__('state', 'Paused')), 'R3', 'FAIL', 'not Playing'),
    ('R3 covers gone', edit('r3.json', lambda d: d['samples'][50].__setitem__('texturesUploaded', 0)), 'R3', 'FAIL', 'covers'),
    ('R3 no minute 30 samples', edit('r3.json', lambda d: [r.__setitem__('m', min(r['m'], 28.9)) for r in d['samples']]), 'R3', 'FAIL', 'no samples at minute'),
    ('R3 PSS growth with upward drift is VOID', both(grow('pss', 11.0), drift(+6.0)), 'R3', 'VOID', 'drifted +6.0'),
    ('R3 PASS with downward drift is VOID', drift(-6.0), 'R3', 'VOID', 'drifted -6.0'),
    ('R3 USS growth with upward drift stands', both(grow('uss', 11.0), drift(+6.0)), 'R3', 'FAIL', 'USS growth 11.0'),
    ('R3 missing reference', remove('reference.json'), 'R3', 'FAIL', 'reference.json'),
    ('R3 guard kill', put('shipped.guard.json', killed='linked to spdif'), 'R3', 'VOID', 'guard killed'),
    ('R4 busy', put('r4.json', cpu=[5.5] * 30), 'R4', 'FAIL', 'CPU median 5.5'),
    ('R4 at the bar', put('r4.json', cpu=[5.0] * 30), 'R4', 'PASS', None),
    ('R4 not playing', put('r4.json', after='Paused'), 'R4', 'FAIL', 'not Playing'),
    ('R4 short window', put('r4.json', cpu=[1.0] * 29), 'R4', 'FAIL', '29 CPU samples'),
    ('R4 two-second bins', put('r4.json', binS=2), 'R4', 'FAIL', 'bins of 2 s'),
    ('R5 a child process', edit('r5.json', lambda d: d['procs'].__setitem__(4, 2)), 'R5', 'FAIL', 'not exactly 1 process'),
    ('R5 procs unrecorded', put('r5.json', procs=[]), 'R5', 'FAIL', 'not exactly 1 process'),
    ('R5 at the bar', put('r5.json', cpu=[1.0] * 30), 'R5', 'PASS', None),
    ('R5 busy', put('r5.json', cpu=[1.5] * 30), 'R5', 'FAIL', 'CPU median 1.5'),
    ('R5 not paused', put('r5.json', before='Playing'), 'R5', 'FAIL', 'not Paused'),
    ('R5 is judged against its own bar', put('r5.json', cpu=[4.0] * 30), 'R5', 'FAIL', 'CPU median 4.0'),
    ('R6 unrecorded', edit('build.json', lambda d: d.pop('shippedBytes')), 'R6', 'FAIL', 'shippedBytes'),
] + [(f'{c} missing evidence', remove(f'{f}.json'), c, 'FAIL', f'{f}.json') for c, files in gate.EVIDENCE.items() for f in files] + [
    (f'{c} default sink changed', put('session.verdict.json', defaultAtEnd='hdmi'), c, 'VOID', 'default sink') for c in gate.CHECKS if c != 'R6'] + [
    (f'{c} co-tenant', put('cotenants.json', seen=[[1500.0, ['123 bramley []']]]), c, 'VOID', 'another weston') for c in gate.CHECKS if c != 'R6']
BALLAST = [
    ('ballast passes the shipped check', put('build.json', ballastFeaturesExit=2), 'failsShippedCheck', 'FAIL'),
    ('ballast fails for another reason', edit('ballast.r1.json', lambda d: d['samples'][2].__setitem__('texturesUploaded', 0)), 'thresholdOnly', 'FAIL'),
    ('ballast fails only for another reason', both(every('ballast', 'uss', 150.0), every('ballast', 'rss', 199.0), edit('ballast.r1.json', lambda d: d['samples'][2].__setitem__('procs', 2))), 'thresholdOnly', 'FAIL'),
    ('ballast lands off band', every('ballast', 'uss', 225.0), 'landedWithinBand', 'FAIL'),
    ('ballast lands off band with a matching rise', both(every('ballast', 'uss', 222.0), put('ballast.r1.json', ballastMiB=73, ballastTouched=73)), 'landedWithinBand', 'FAIL'),
    ('ballast did not raise USS', put('ballast.r1.json', ballastMiB=40, ballastTouched=40), 'ussRoseByBallast', 'FAIL'),
    ('ballast rise beyond its size', put('ballast.r1.json', ballastMiB=63, ballastTouched=63), 'ussRoseByBallast', 'FAIL'),
    ('ballast windows from two launches', edit('ballast.r1.json', lambda d: d['baseSamples'][0].__setitem__('pid', 8)), 'oneLaunch', 'FAIL'),
    ('ballast without a pre-ballast window', put('ballast.r1.json', baseSamples=[]), 'oneLaunch', 'FAIL'),
    ('ballast never confirmed touched', put('ballast.r1.json', ballastTouched=None), 'touched', 'FAIL'),
    ('ballast touched at another size', put('ballast.r1.json', ballastTouched=66), 'touched', 'FAIL'),
    ('ballast samples without a pid', edit('ballast.r1.json', lambda d: [r.pop('pid') for r in d['baseSamples'] + d['samples']] and d.pop('pid')), 'oneLaunch', 'FAIL'),
    ('ballast windows from another pid', edit('ballast.r1.json', lambda d: [r.__setitem__('pid', 8) for r in d['baseSamples'] + d['samples']]), 'oneLaunch', 'FAIL'),
    ('ballast pre window not playing', edit('ballast.r1.json', lambda d: d['baseSamples'][4].__setitem__('state', 'Paused')), 'preWindowValid', 'FAIL'),
    ('ballast pre window short', edit('ballast.r1.json', lambda d: d['baseSamples'].pop()), 'preWindowValid', 'FAIL'),
    ('ballast pre window without covers', edit('ballast.r1.json', lambda d: d['baseSamples'][4].__setitem__('texturesUploaded', 0)), 'preWindowValid', 'FAIL'),
    ('ballast pre window forked', edit('ballast.r1.json', lambda d: d['baseSamples'][4].__setitem__('procs', 2)), 'preWindowValid', 'FAIL'),
    ('ballast pre window with a banner', edit('ballast.r1.json', lambda d: d['baseSamples'][4].__setitem__('banner', 'Error')), 'preWindowValid', 'FAIL'),
    ('ballast-only void', put('ballast.guard.json', killed='linked to spdif'), 'thresholdOnly', 'VOID'),
]


class Verdict(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)
        os.makedirs(SCRATCH)
        self.addCleanup(shutil.rmtree, SCRATCH, True)

    def matrix(self, cases):
        for name, mutate, check, want, reason in cases:
            with self.subTest(name):
                write_valid(SCRATCH)
                mutate(SCRATCH)
                got = gate.verdict(SCRATCH)['checks'][check]
                self.assertEqual(got['verdict'], want, got['reasons'])
                self.assertTrue(reason is None or any(reason in r for r in got['reasons']), got['reasons'])

    def test_an_empty_evidence_directory_fails_every_check_and_the_gate(self):
        v = gate.verdict(SCRATCH)
        self.assertEqual({c['verdict'] for c in v['checks'].values()} | {v['gate']}, {'FAIL'})
        self.assertFalse(v['ballast']['holds'])

    def test_complete_evidence_passes_every_check_and_the_ballast_holds_in_zone_1(self):
        write_valid(SCRATCH)
        v = gate.verdict(SCRATCH)
        self.assertEqual({n: c['verdict'] for n, c in v['checks'].items() if c['verdict'] != 'PASS'}, {})
        self.assertEqual((v['checks']['R1']['zone'], v['ballast']['zone'], v['ballast']['holds'], v['gate']), (2, 1, True, 'PASS'))

    def test_each_memory_rule_moves_its_check_with_its_own_reason(self):
        self.matrix(MEMORY)

    def test_each_functional_and_resource_rule_moves_its_check_with_its_own_reason(self):
        self.matrix(FUNCTIONAL)

    def test_the_gate_is_void_on_any_void_even_beside_a_fail_and_fail_on_any_fail(self):
        for name, mutate, want in (('R1 VOID with R2 FAIL', both(put('shipped.guard.json', log=[]), every('shipped', 'uss', 101.0, 'r2')), 'VOID'),
                                   ('one F check FAIL', put('f8.json', seen=[]), 'FAIL'), ('one F check VOID', put('f1.json', renderers=[LLVMPIPE]), 'VOID'),
                                   ('ballast fails, all checks pass', put('ballast.r1.json', ballastMiB=40), 'FAIL')):
            with self.subTest(name):
                write_valid(SCRATCH)
                mutate(SCRATCH)
                self.assertEqual(gate.verdict(SCRATCH)['gate'], want)

    def test_each_ballast_clause_breaks_the_control_and_the_gate(self):
        for name, mutate, clause, gate_verdict in BALLAST:
            with self.subTest(name):
                write_valid(SCRATCH)
                mutate(SCRATCH)
                v = gate.verdict(SCRATCH)
                self.assertEqual((v['ballast'][clause], v['ballast']['holds'], v['gate']), (False, False, gate_verdict))

    def test_the_ballast_rise_is_measured_within_its_own_launch(self):
        write_valid(SCRATCH)
        kept = 50 * 96 * 96 * 4
        edit('ballast.r1.json', lambda d: d.update(baseSamples=[dict(r, decodedBytesKept=kept) for r in d['baseSamples']],
                                                   samples=[dict(r, decodedBytesKept=kept) for r in d['samples']]))(SCRATCH)
        every('shipped', 'uss', 140.0)(SCRATCH)
        v = gate.verdict(SCRATCH)['ballast']
        self.assertEqual((v['keptAddedMiB'], v['ussRise'], v['holds']), (0.0, 67.0, True))
        edit('ballast.r1.json', lambda d: d.update(ballastMiB=63, ballastTouched=63, baseSamples=[dict(r, decodedBytesKept=0) for r in d['baseSamples']]))(SCRATCH)
        v = gate.verdict(SCRATCH)['ballast']
        self.assertEqual((v['keptAddedMiB'], v['ussRoseByBallast']), (1.76, True))
        put('ballast.r1.json', ballastMiB=62, ballastTouched=62)(SCRATCH)
        self.assertFalse(gate.verdict(SCRATCH)['ballast']['ussRoseByBallast'])

    def test_a_touched_real_token_or_keyring_or_a_missing_record_fails_the_gate(self):
        for name, mutate in (('mtime', edit('realstate.json', lambda d: d['after'].__setitem__('tokenMtimeNs', 2))),
                             ('keyring', edit('realstate.json', lambda d: d['after'].__setitem__('keyringItems', ['/item/1']))),
                             ('cache', edit('realstate.json', lambda d: d['after'].__setitem__('cache', {'developer-token.json': [2, 300]}))),
                             ('config only', edit('realstate.json', lambda d: d['after'].__setitem__('config', {'developer-token': [3, 2]}))),
                             ('unread keyring', edit('realstate.json', lambda d: [d[k].__setitem__('keyringItems', None) for k in d])),
                             ('missing', remove('realstate.json')), ('a name in the run', put('leakgrep.json', hits={'repo': 0, 'run': 1})),
                             ('a name in the repo', put('leakgrep.json', hits={'repo': 2, 'run': 0})), ('a run grep that cannot fail', put('leakgrep.json', plantedRun=False)),
                             ('a repo grep that cannot fail', put('leakgrep.json', plantedRepo=False)), ('an old single-plant record', put('leakgrep.json', plantedRun=None, plantedRepo=None, plantedCaught=True)),
                             ('no pacing record', remove('pacing.json')),
                             ('21 actions in a window', put('pacing.json', actions=[[1000.0 + 20 * i, 'play'] for i in range(21)], prior=[])),
                             ('a prior action makes 21', put('pacing.json', prior=[995.0])),
                             ('no grep', remove('leakgrep.json')), ('a grep that read no files', put('leakgrep.json', runFiles=0)), ('a grep with no library names', put('leakgrep.json', libraryNames=0)),
                             ('a phase raised', lambda out: gate.save(out, 'errors.json', {'browse': 'RuntimeError: boom'}))):
            with self.subTest(name):
                write_valid(SCRATCH)
                mutate(SCRATCH)
                v = gate.verdict(SCRATCH)
                self.assertEqual((v['realStateUnchanged'] and v['leakGrep']['clean'] and not v['phaseErrors'] and v['pacing']['ok'], v['gate']), (False, 'FAIL'))

    def test_a_phase_skipped_after_a_guard_kill_is_not_a_phase_error(self):
        write_valid(SCRATCH)
        gate.save(SCRATCH, 'errors.json', {'browse': "RuntimeError: skipped: the guard fired in ['shipped.guard.json']"})
        v = gate.verdict(SCRATCH)
        self.assertEqual((v['phaseErrors'], v['gate']), ({}, 'PASS'))

    def test_llvmpipe_voids_the_named_checks_and_the_control_needs_llvmpipe_seen(self):
        write_valid(SCRATCH, renderer=LLVMPIPE, mode='llvmpipe')
        put('f1.json', renderers=[LLVMPIPE] * 5)(SCRATCH)
        v = gate.verdict(SCRATCH)
        self.assertEqual(([v['checks'][c]['verdict'] for c in ('F1', 'F4', 'R1', 'R2')], v['llvmpipeControl']['holds'], v['gate']), (['VOID'] * 4, True, 'PASS'))
        for name, mutate in (('a GPU that is not radeonsi', lambda out: [every('shipped', 'glRenderer', 'Some GPU (nouveau)', k)(out) for k in ('r1', 'r2')]),
                             ('F1 on the GPU', put('f1.json', renderers=[RADEONSI] * 5)), ('missing R1', remove('shipped.r1.json'))):
            with self.subTest(name):
                write_valid(SCRATCH, renderer=LLVMPIPE, mode='llvmpipe')
                put('f1.json', renderers=[LLVMPIPE] * 5)(SCRATCH)
                mutate(SCRATCH)
                v = gate.verdict(SCRATCH)
                self.assertEqual((v['llvmpipeControl']['holds'], v['gate']), (False, 'FAIL'))


def write_controls(out):
    write_valid(out, mode='controls')
    shutil.copy(os.path.join(out, 'f6.json'), os.path.join(out, 'cut.f6.json'))
    edit('cut.f6.json', lambda d: [d['transitions'][2]['end'].__setitem__('skippedMs', 2900), d['transitions'][2].__setitem__('cut', True)])(out)
    for f in os.listdir(out):
        if f.split('.')[0] in ('f2', 'f3', 'f4', 'f5', 'f6', 'f9', 'r3', 'shipped'):
            shutil.copy(os.path.join(out, f), os.path.join(out, ('leak.' if f.startswith('r3') else 'nouser.') + f))
    for pre in ('badtoken.', 'nostream.'):
        for f in ('f3.json', 'f4.json'):
            shutil.copy(os.path.join(out, f), os.path.join(out, pre + f))
    never = lambda banner: dict(started=False, reps=[{'audioMs': None, 'exit': 0, 'banner': banner}] * 5, judged=[])
    put('nouser.f4.json', **never('SignInNeeded'))(out)
    put('nouser.f5.json', rounds=[], started=False)(out)
    put('nouser.f6.json', transitions=[], started=False)(out)
    put('nouser.f9.json', **never('SignInNeeded'))(out)
    put('nouser.shipped.r1.json', started=False, samples=[])(out)
    put('badtoken.f3.json', reps=[{'exit': 1, 'wallMs': 300, 'songs': 0, 'uiMs': None, 'error': "Bramley's developer token was rejected"}] * 5)(out)
    put('badtoken.f4.json', **never('Error("Bramley\'s developer token was rejected")'))(out)
    put('nostream.f4.json', **never('Error("the audio output could not be opened")'))(out)
    edit('leak.r3.json', lambda d: [r.__setitem__('uss', r['uss'] + 3 * int(r['itemId'][1:])) for r in d['samples']])(out)
    gate.save(out, 'stall.stall.json', {'before': counts(1000.0, 37.0, 38000), 'after': counts(1010.0, 44.1, 45000), 'stoppedS': 3})
    edit('stall.stall.json', lambda d: [d[k].__setitem__('itemId', SONG) for k in ('before', 'after')])(out)
    edit('stall.stall.json', lambda d: d['after'].__setitem__('outputUnderflows', 1))(out)
    gate.save(out, 'guardtest.json', {'defaultBefore': 'iec958', 'defaultAfter': 'iec958', 'wrongSinkPresent': True, 'appGone': True, 'wrongSinkSeen': True,
                                      'killed': "stream linked to ['bramley-gate-wrong-null-x']"})


class LeakGrep(unittest.TestCase):
    def test_names_match_as_whole_words_only(self):
        self.assertEqual(gate.count_names('worktrees and trees; tree-trees', ['Trees']), 2)
        self.assertEqual(gate.count_names('see trees', ['trees', 'see trees']), 2)

    def test_only_single_dictionary_words_and_allowlisted_hashes_are_exempt(self):
        words, digest = {'running', 'first', 'time'}, __import__('hashlib').sha256(b'first time').hexdigest()
        self.assertTrue(gate.leak_exempt('Running', words, set()))
        self.assertFalse(gate.leak_exempt('First Time', words, set()))
        self.assertTrue(gate.leak_exempt('First Time', words, {digest}))
        self.assertTrue(gate.leak_exempt('FIRST TIME', set(), {digest}))
        self.assertFalse(gate.leak_exempt('Zxqv', words, {digest}))

    def test_the_allowlist_is_read_as_hashes_and_a_missing_file_exempts_nothing(self):
        os.makedirs(SCRATCH, exist_ok=True)
        self.addCleanup(shutil.rmtree, SCRATCH, True)
        path = os.path.join(SCRATCH, 'allow.sha256')
        open(path, 'w').write('aa11 catalog-constant\n\nbb22 ordinary-phrase\n')
        self.assertEqual((gate.allowlisted(path), gate.allowlisted(path + '.gone')), ({'aa11', 'bb22'}, set()))

    def test_the_committed_allowlist_holds_hashes_and_kinds_only(self):
        rows = [line.split() for line in open(os.path.join(HERE, 'leak-allowlist.sha256')) if line.strip()]
        self.assertTrue(rows)
        self.assertTrue(all(len(r) == 2 and len(r[0]) == 64 and r[1] in ('catalog-constant', 'ordinary-phrase', 'test-constant') for r in rows), rows)

    def test_the_repo_plant_works_on_an_empty_diff_and_needs_exactly_one_more_hit(self):
        self.assertTrue(gate.repo_plant('', ['secret one'], 0))
        self.assertTrue(gate.repo_plant('notes about secret one', ['secret one'], 1))
        self.assertFalse(gate.repo_plant('notes about secret one', ['secret one'], 0))
        self.assertFalse(gate.repo_plant('', [], 0))


def load_script(name, *path):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, '..', '..', *path))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ColdCache(unittest.TestCase):
    def setUp(self):
        self.drive = load_script('drive_under_test', 'scripts', 'acceptance', 'drive.py')
        self.cs = load_script('covers_session_under_test', 'scripts', 'app', 'covers-session.py')
        os.makedirs(SCRATCH, exist_ok=True)
        self.addCleanup(shutil.rmtree, SCRATCH, True)

    def test_a_relaunch_in_one_session_starts_without_art_or_api_cache_and_keeps_the_developer_token(self):
        root = os.path.join(SCRATCH, 'cache', 'bramley')
        for sub in ('art', 'api'):
            os.makedirs(os.path.join(root, sub))
            open(os.path.join(root, sub, 'entry'), 'w').write('cached')
        open(os.path.join(root, 'developer-token.json'), 'w').write('{}')
        self.drive.cold_cache({'XDG_CACHE_HOME': os.path.join(SCRATCH, 'cache')})
        self.assertEqual(os.listdir(root), ['developer-token.json'])

    def test_a_launch_that_would_use_the_real_cache_is_refused(self):
        base = {'BRAMLEY_CONFIG_DIR': os.path.join(SCRATCH, 'config'), 'BRAMLEY_KEYRING_SERVICE': 'bramley-gate-t', 'XDG_STATE_HOME': os.path.join(SCRATCH, 'config', 'state')}
        self.cs.assert_isolated(dict(base, XDG_CACHE_HOME=os.path.join(SCRATCH, 'config', 'cache')))
        for cache in (os.path.dirname(self.cs.REAL_CACHE), None):
            with self.assertRaises(RuntimeError):
                self.cs.assert_isolated(dict(base, **({} if cache is None else {'XDG_CACHE_HOME': cache})))

    def test_a_launch_that_could_read_the_host_omarchy_theme_is_refused(self):
        config = os.path.join(SCRATCH, 'config')
        base = {'BRAMLEY_CONFIG_DIR': config, 'BRAMLEY_KEYRING_SERVICE': 'bramley-gate-t', 'XDG_CACHE_HOME': os.path.join(config, 'cache')}
        self.cs.assert_isolated(dict(base, XDG_STATE_HOME=os.path.join(config, 'state')))
        for state in (None, '', os.path.expanduser('~/.local/state'), os.path.dirname(config)):
            with self.assertRaises(RuntimeError):
                self.cs.assert_isolated(dict(base, **({} if state is None else {'XDG_STATE_HOME': state})))

    def test_every_launch_in_the_driver_goes_through_the_cold_cache_wipe(self):
        text = open(os.path.join(HERE, 'drive.py')).read()
        self.assertEqual(text.count('subprocess.Popen([*wrap, binary'), 1)
        init = text[text.index('class App:'):text.index('self.log = open')]
        self.assertIn('cs.assert_isolated(env)\n        cold_cache(env)', init)


class Clock:
    def __init__(self, now=10000.0):
        self.now, self.slept = now, []

    def __call__(self):
        return self.now

    def sleep(self, s):
        self.slept.append(round(s, 3))
        self.now += s


class Pacing(unittest.TestCase):
    def pacer(self, prior=(), now=10000.0):
        clock, kept = Clock(now), []
        return gate.Pacer(prior, clock=clock, sleep=clock.sleep, keep=kept.append), clock, kept

    def test_twenty_actions_go_at_once_and_the_twenty_first_waits_for_the_oldest_to_leave_the_window(self):
        p, clock, kept = self.pacer()
        for i in range(20):
            p.take('play')
            clock.now += 1
        self.assertEqual(clock.slept, [])
        p.take('next')
        self.assertEqual((clock.slept, p.waits[0][1:], clock.now), ([580.001], [580.001, 20], 10600.001))
        self.assertEqual((len(p.actions), gate.peak_in_window(p.times())), (21, 20))
        self.assertEqual(kept[-1], p.times())

    def test_a_wait_always_moves_past_the_boundary_even_where_float_rounding_would_stall(self):
        clock, n = [0.0], [0]

        def sleep(w):
            n[0] += 1
            clock[0] += w
            self.assertLess(n[0], 100, 'the pacer stopped making progress')

        p, t = gate.Pacer(clock=lambda: clock[0], sleep=sleep), 40.0
        for _ in range(40):
            clock[0] = max(clock[0], t)
            p.take('play')
            t += 3.7
        self.assertEqual(gate.peak_in_window(p.times()), 20)

    def test_prior_actions_from_earlier_runs_count(self):
        p, clock, _ = self.pacer(prior=[9500.0 + i for i in range(19)])
        p.take('play')
        self.assertEqual(clock.slept, [])
        p.take('play')
        self.assertEqual(clock.slept, [100.001])

    def test_an_action_exactly_one_window_old_no_longer_counts(self):
        p, clock, _ = self.pacer(prior=[9400.0] + [9900.0] * 19)
        p.take('play')
        self.assertEqual(clock.slept, [])

    def test_a_reservation_waits_for_room_without_recording_and_a_note_never_waits(self):
        p, clock, _ = self.pacer(prior=[9500.0 + i for i in range(16)])
        p.room(gate.R3_RESERVE)
        self.assertEqual((clock.slept, p.actions), ([101.001], []))
        q, qclock, _ = self.pacer(prior=[9500.0] * 20)
        q.note('item-change', 2)
        self.assertEqual((qclock.slept, len(q.actions)), ([], 2))

    def test_only_commands_that_start_an_item_are_counted(self):
        cases = {('play', '1'): 'play', ('play',): None, ('next',): 'next', ('prev',): 'prev', ('jump',): 'jump', ('seek', '120'): None,
                 ('queue', 'add', '1'): None, ('pause',): None, ('now',): None, ('open', 'home'): None}
        self.assertEqual({a: gate.license_kind(a) for a in cases}, cases)

    def test_the_window_peak_counts_actions_after_the_window_start_up_to_and_including_its_end(self):
        self.assertEqual(gate.peak_in_window([0.0, 600.0, 600.0]), 2)
        self.assertEqual(gate.peak_in_window([0.0, 599.9]), 2)
        self.assertEqual(gate.peak_in_window([]), 0)


class PacingVerdict(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)
        os.makedirs(SCRATCH)
        self.addCleanup(shutil.rmtree, SCRATCH, True)

    def test_the_verdict_reports_waits_and_the_peak_and_names_a_harness_defect_above_twenty(self):
        write_valid(SCRATCH)
        p = gate.verdict(SCRATCH)['pacing']
        self.assertEqual({k: p[k] for k in ('ok', 'peak', 'actions', 'waits', 'waitS')}, {'ok': True, 'peak': 20, 'actions': 20, 'waits': 1, 'waitS': 120.5})
        put('pacing.json', prior=[995.0])(SCRATCH)
        p = gate.verdict(SCRATCH)['pacing']
        self.assertEqual((p['ok'], p['peak'], p['reasons']), (False, 21, ['harness defect: 21 license-requesting actions in 600 s > 20']))


def soak_rows(n=721):
    return [{'t': 1000.0 + 10 * i, 'm': round(i / 6, 3), 'pid': 7, 'procs': 1, 'pss': 170.0, 'uss': 148.0, 'rss': 205.0, 'texturesUploaded': 50, 'state': 'Playing', 'banner': None,
             'itemId': f'i{i // 16}', 'position': 20.0 + 10 * (i % 16), 'duration': 180.0, 'outputUnderflows': 3, 'readinessDips': 5} for i in range(n)]


def write_soak(out):
    write_valid(out, mode='soak')
    put('build.json', soakMinutes=120)(out)
    gate.save(out, 'soak.shipped.guard.json', {'pid': 7, 'killed': None, 'log': [[990.0 + i / 4, [SINK]] for i in range(400)], 'modes': OFF})
    gate.save(out, 'soak.r3.json', {'pid': 7, 't0': 1000.0, 't1': 8210.0, 'session': 'soak.shipped', 'renderers': [RADEONSI], 'samples': soak_rows(),
                                    'changes': [[1000.0 + 160 * i, f'i{i}'] for i in range(45)], 'stalls': [], 'refills': [3000.0, 5500.0], 'soakS': 7200})


def soak_edit(fn):
    return edit('soak.r3.json', lambda d: fn(d))


def freeze(first, count):
    return soak_edit(lambda d: [d['samples'][i].__setitem__('position', 77.0) or d['samples'][i].__setitem__('itemId', 'x') for i in range(first, first + count)])


SOAK = [
    ('valid soak', lambda out: None, 'PASS', None),
    ('a driver stall', soak_edit(lambda d: d.__setitem__('stalls', [{'t0': 5000.0, 't1': 5031.0, 'item': 'i3', 'position': 4.0, 'state': 'Loading'}])), 'FAIL', 'permanent stall (race 3 reopens)'),
    ('frozen snapshots over 30 s', freeze(100, 5), 'FAIL', '1 frozen snapshot runs'),
    ('frozen snapshots at exactly 30 s', freeze(100, 4), 'PASS', None),
    ('frozen while Paused is not a stall', both(freeze(100, 8), soak_edit(lambda d: [d['samples'][i].__setitem__('state', 'Paused') for i in range(100, 108)])), 'PASS', None),
    ('frozen while Loading', both(freeze(100, 6), soak_edit(lambda d: [d['samples'][i].__setitem__('state', 'Loading') for i in range(100, 106)])), 'FAIL', 'permanent stall'),
    ('stall record missing', soak_edit(lambda d: d.pop('stalls')), 'FAIL', 'stall record missing'),
    ('29 item changes', soak_edit(lambda d: d.__setitem__('changes', d['changes'][:29])), 'FAIL', '29 item changes < 30'),
    ('30 item changes', soak_edit(lambda d: d.__setitem__('changes', d['changes'][:30])), 'PASS', None),
    ('item changes unrecorded', soak_edit(lambda d: d.pop('changes')), 'FAIL', 'None item changes'),
    ('an output underflow', soak_edit(lambda d: d['samples'][-1].__setitem__('outputUnderflows', 4)), 'FAIL', 'output underflows during the soak: 1'),
    ('output underflows unreadable', soak_edit(lambda d: d['samples'][-1].pop('outputUnderflows')), 'FAIL', 'outputUnderflows unreadable'),
    ('a readiness dip', soak_edit(lambda d: d['samples'][-1].__setitem__('readinessDips', 6)), 'FAIL', 'readiness dips during the soak: 1'),
    ('readiness dips unreadable', soak_edit(lambda d: d['samples'][0].pop('readinessDips')), 'FAIL', 'readinessDips unreadable'),
    ('another pid', soak_edit(lambda d: d['samples'][300].__setitem__('pid', 8)), 'FAIL', 'crashed, restarted or forked'),
    ('a child process', soak_edit(lambda d: d['samples'][300].__setitem__('procs', 2)), 'FAIL', 'crashed, restarted or forked'),
    ('ended 20 minutes early', soak_edit(lambda d: d.__setitem__('samples', d['samples'][:600])), 'FAIL', 'soak ended at minute 99.833 < 120'),
    ('ended within one snapshot period', soak_edit(lambda d: d['samples'][-1].__setitem__('m', 119.84)), 'PASS', None),
    ('ended just past the tolerance', soak_edit(lambda d: d['samples'][-1].__setitem__('m', 119.8)), 'FAIL', 'soak ended at minute 119.8 < 120'),
    ('no soak evidence', remove('soak.r3.json'), 'FAIL', 'missing evidence: soak.r3.json'),
    ('no guard record', remove('soak.shipped.guard.json'), 'FAIL', 'soak.shipped.guard.json'),
    ('a soak that started shuffled', put('soak.shipped.guard.json', modes=dict(OFF, shuffleMode='on')), 'VOID', 'shuffle and repeat off'),
    ('guard killed the app', put('soak.shipped.guard.json', killed='linked to spdif'), 'VOID', 'guard killed'),
    ('not radeonsi', soak_edit(lambda d: d.__setitem__('renderers', [LLVMPIPE])), 'VOID', 'renderer'),
    ('a reference on the wrong sha', put('reference.json', sha256='feed'), 'VOID', 'sha'),
    ('positions carried across items', soak_edit(lambda d: [r.__setitem__('position', r['position'] + 180.0 * int(r['itemId'][1:])) for r in d['samples']]), 'FAIL', 'position beyond duration + 1.0 s in'),
    ('position at duration + 1', soak_edit(lambda d: d['samples'][50].__setitem__('duration', d['samples'][50]['position'] - 1.0)), 'PASS', None),
    ('position past duration + 1', soak_edit(lambda d: d['samples'][50].__setitem__('duration', d['samples'][50]['position'] - 1.1)), 'FAIL', 'worst +1.1 s on item i3'),
    ('a Playing sample without a duration', soak_edit(lambda d: d['samples'][50].pop('duration')), 'FAIL', '1 Playing samples without a position or duration'),
    ('a Playing sample without a position', soak_edit(lambda d: d['samples'][50].__setitem__('position', None)), 'FAIL', '1 Playing samples without a position or duration'),
    ('a Paused sample past the end is not judged', soak_edit(lambda d: d['samples'][50].update(state='Paused', duration=1.0)), 'PASS', None),
    ('banners are information only', soak_edit(lambda d: d['samples'][50].__setitem__('banner', 'Error')), 'PASS', None),
]


class FrozenRuns(unittest.TestCase):
    def rows(self, span):
        return [{'t': 1000.0, 'itemId': 'a', 'position': 5.0, 'state': 'Playing'}, {'t': 1000.0 + span, 'itemId': 'a', 'position': 5.0, 'state': 'Playing'},
                {'t': 1001.0 + span, 'itemId': 'a', 'position': 6.0, 'state': 'Playing'}]

    def test_a_run_counts_only_when_it_spans_more_than_thirty_seconds(self):
        self.assertEqual([len(gate.frozen_runs(self.rows(s))) for s in (29.9, 30.0, 30.5, 32.0)], [0, 0, 1, 1])

    def test_a_frozen_pause_is_not_a_run(self):
        rows = [dict(r, state='Paused') for r in self.rows(100.0)]
        self.assertEqual(gate.frozen_runs(rows), [])


class Soak(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)
        os.makedirs(SCRATCH)
        self.addCleanup(shutil.rmtree, SCRATCH, True)

    def test_each_soak_rule_moves_the_soak_with_its_own_reason(self):
        for name, mutate, want, reason in SOAK:
            with self.subTest(name):
                write_soak(SCRATCH)
                mutate(SCRATCH)
                got = gate.verdict(SCRATCH)['soak']
                self.assertEqual(got['verdict'], want, got['reasons'])
                self.assertTrue(reason is None or any(reason in r for r in got['reasons']), got['reasons'])

    def test_growth_is_read_from_minute_five_not_from_the_start(self):
        write_soak(SCRATCH)
        soak_edit(lambda d: [r.__setitem__('pss', 160.0) for r in d['samples'] if r['m'] < 1])(SCRATCH)
        self.assertEqual(gate.verdict(SCRATCH)['soak']['pss'], 0.0)

    def test_a_passing_soak_reports_growth_counters_and_never_counts_toward_v1(self):
        write_soak(SCRATCH)
        soak_edit(lambda d: [r.__setitem__('pss', 170.0 + (3.0 if r['m'] >= 100 else 0)) for r in d['samples']])(SCRATCH)
        v = gate.verdict(SCRATCH)
        s = v['soak']
        self.assertEqual((v['gate'], v['countsTowardV1'], 'checks' in v), ('PASS', False, False))
        self.assertEqual((s['pss'], s['itemChanges'], s['stalls'], s['frozenRuns'], s['refills'], s['outputUnderflows'], s['readinessDips'], s['minutes']), (3.0, 45, 0, 0, 2, 0, 0, 120.0))
        self.assertEqual(s['growthPerHour']['pss'], round(3.0 / ((120.0 - 5) / 60), 2))
        self.assertEqual((s['bannerSamples'], s['maxPositionOverrunS']), (0, -10.0))

    def test_the_soak_gate_needs_pacing_and_a_void_soak_is_a_void_gate(self):
        write_soak(SCRATCH)
        put('pacing.json', prior=[995.0])(SCRATCH)
        self.assertEqual(gate.verdict(SCRATCH)['gate'], 'FAIL')
        write_soak(SCRATCH)
        put('soak.shipped.guard.json', killed='linked to spdif')(SCRATCH)
        self.assertEqual(gate.verdict(SCRATCH)['gate'], 'VOID')
        write_soak(SCRATCH)
        soak_edit(lambda d: d.__setitem__('stalls', [{'t0': 1.0}]))(SCRATCH)
        self.assertEqual(gate.verdict(SCRATCH)['gate'], 'FAIL')

    def test_a_counted_run_has_no_soak_section(self):
        write_valid(SCRATCH)
        v = gate.verdict(SCRATCH)
        self.assertEqual(('soak' in v, v.get('countsTowardV1')), (False, None))


def sample(t, item, position, duration, state='Playing'):
    return {'t': t, 'itemId': item, 'state': state, 'position': position, 'duration': duration}


class SeekSettle(unittest.TestCase):
    def pair(self):
        return [sample(100.0, 'a', 10.0, 200.0), sample(110.0, 'a', 10.0, 200.0)]

    def test_a_pair_14_9_s_after_a_seek_is_excluded(self):
        self.assertEqual(gate.lags(self.pair(), [85.1]), [])

    def test_a_pair_15_s_after_a_seek_is_judged(self):
        self.assertEqual(gate.lags(self.pair(), [85.0]), [10.0])

    def test_a_pair_the_seek_falls_inside_is_excluded_as_before(self):
        self.assertEqual(gate.lags(self.pair(), [105.0]), [])

    def test_a_pair_before_the_seek_is_judged(self):
        self.assertEqual(gate.lags(self.pair(), [111.1]), [10.0])


class SeekLanding(unittest.TestCase):
    def landing(self, position, t=1010.0, item='a'):
        return gate.seek_landing([sample(1000.0, 'a', 70.0, 200.0), sample(t, item, position, 200.0)], 1000.0)

    def test_the_landing_is_the_wall_time_less_the_playback_past_the_target(self):
        self.assertEqual(self.landing(194.0), 6.0)

    def test_the_first_sample_not_past_the_target_by_more_than_the_epsilon_does_not_land(self):
        self.assertIsNone(self.landing(190.05))
        self.assertEqual(self.landing(190.06), 9.94)

    def test_a_sample_on_another_item_is_not_a_landing(self):
        self.assertIsNone(self.landing(194.0, item='b'))

    def test_the_search_ends_at_the_first_item_change(self):
        rows = [sample(1000.0, 'a', 70.0, 200.0), sample(1005.0, 'b', 1.0, 200.0), sample(1010.0, 'a', 194.0, 200.0)]
        self.assertIsNone(gate.seek_landing(rows, 1000.0))

    def test_a_seek_with_no_sample_before_it_has_no_landing(self):
        self.assertIsNone(gate.seek_landing([sample(1010.0, 'a', 194.0, 200.0)], 1000.0))

    def test_the_target_is_the_one_the_driver_seeks_to(self):
        self.assertEqual(gate.seek_target(243.863), 233.9)


class Replay(unittest.TestCase):
    RECORDED = '/home/norm/projects/Personal/bramley-wt-v012/target/acceptance/v012-counted-1/r3.json'

    @unittest.skipUnless(os.path.exists(RECORDED), 'the recorded run is not on this machine')
    def test_v012_counted_1_r3_passes_with_a_max_landing_of_5_06(self):
        with open(self.RECORDED) as f:
            ev = json.load(f)
        fails = []
        out = gate.r3_rules(ev, fails, None)
        self.assertEqual((fails, max(out['seekLandingsS']), len(out['seekLandingsS'])), ([], 5.06, 8))


class SeamSample(unittest.TestCase):
    def overruns(self, rows):
        fails = []
        gate.position_overruns(rows, fails)
        return fails

    def test_a_seam_sample_with_a_shorter_new_item_is_judged_on_the_old_duration(self):
        rows = [sample(1000.0, 'a', 172.6, 172.9), sample(1000.25, 'b', 172.85, 140.0)]
        self.assertEqual(self.overruns(rows), [])

    def test_a_seam_sample_with_a_longer_new_item_is_not_a_stall(self):
        rows = [sample(1000.0, 'a', 172.6, 172.9), sample(1000.25, 'b', 172.85, 200.0), sample(1010.25, 'b', 9.5, 200.0)]
        self.assertEqual(gate.lags(rows, []), [])

    def test_a_first_sample_that_does_not_continue_the_old_clock_is_judged_on_its_own_duration(self):
        rows = [sample(1000.0, 'a', 172.6, 172.9), sample(1000.25, 'b', 150.0, 140.0)]
        self.assertEqual(len(self.overruns(rows)), 1)
        self.assertIn('worst +10.0 s on item b', self.overruns(rows)[0])

    def test_the_sample_after_a_seam_sample_is_judged_on_its_own_duration(self):
        rows = [sample(1000.0, 'a', 172.6, 172.9), sample(1000.25, 'b', 172.85, 140.0), sample(1010.25, 'b', 182.85, 140.0)]
        self.assertIn('in 1 of 3 Playing samples, worst +42.8 s', self.overruns(rows)[0])

    def test_a_first_sample_after_a_sample_that_is_not_playing_is_not_a_seam_sample(self):
        rows = [sample(1000.0, 'a', 172.6, 172.9, 'Paused'), sample(1000.25, 'b', 172.85, 140.0)]
        self.assertEqual(len(self.overruns(rows)), 1)

    def test_a_sample_at_the_same_item_is_not_a_seam_sample(self):
        rows = [sample(1000.0, 'a', 172.6, 172.9), sample(1000.25, 'a', 172.85, 140.0)]
        self.assertEqual(len(self.overruns(rows)), 1)

    def test_the_clock_must_continue_to_within_the_pace_tolerance(self):
        within = [sample(1000.0, 'a', 171.75, 173.0), sample(1000.25, 'b', 173.0, 140.0)]
        beyond = [sample(1000.0, 'a', 171.75, 173.0), sample(1000.25, 'b', 173.1, 140.0)]
        self.assertEqual((len(self.overruns(within)), len(self.overruns(beyond))), (0, 1))

    def test_a_position_below_the_old_clock_by_more_than_the_pace_tolerance_is_not_a_seam_sample(self):
        within = [sample(1000.0, 'a', 171.75, 173.0), sample(1000.25, 'b', 171.0, 140.0)]
        beyond = [sample(1000.0, 'a', 171.75, 173.0), sample(1000.25, 'b', 170.9, 140.0)]
        self.assertEqual((len(self.overruns(within)), len(self.overruns(beyond))), (0, 1))

    def test_an_unreadable_sample_before_an_item_change_is_reported_not_a_crash(self):
        for old in (sample(1000.0, 'a', None, 172.9), sample(1000.0, 'a', 172.6, None)):
            self.assertEqual(len(self.overruns([old, sample(1000.25, 'b', 172.85, 140.0)])), 2)

    def test_the_old_item_must_be_within_the_seam_tolerance_of_its_end(self):
        at_edge = [sample(1000.0, 'a', 171.75, 173.0), sample(1000.25, 'b', 172.0, 100.0)]
        short = [sample(1000.0, 'a', 171.5, 173.0), sample(1000.25, 'b', 171.75, 100.0)]
        self.assertEqual((len(self.overruns(at_edge)), len(self.overruns(short))), (0, 1))


class StallWatchTests(unittest.TestCase):
    def test_a_stall_opens_after_thirty_seconds_extends_and_a_move_ends_it(self):
        w = gate.StallWatch()
        for t in range(0, 31):
            w.update(100.0 + t, 'Playing', 'a', 5.0)
        self.assertEqual(w.stalls, [])
        w.update(130.5, 'Playing', 'a', 5.0)
        self.assertEqual(len(w.stalls), 1)
        w.update(131.0, 'Playing', 'a', 5.0)
        w.update(140.0, 'Loading', 'a', 5.0)
        self.assertEqual([(s['t0'], s['t1'], s['state']) for s in w.stalls], [(100.0, 140.0, 'Playing')])
        w.update(141.0, 'Playing', 'a', 6.0)
        for t in range(142, 180):
            w.update(float(t), 'Playing', 'a', 6.0)
        self.assertEqual(len(w.stalls), 2)

    def test_paused_stopped_and_moving_playback_never_stall(self):
        w = gate.StallWatch()
        for t in range(0, 200):
            w.update(float(t), 'Paused', 'a', 5.0)
        for t in range(200, 400):
            w.update(float(t), 'Stopped', None, None)
        for t in range(400, 600):
            w.update(float(t), 'Playing', 'a', float(t))
        self.assertEqual(w.stalls, [])

    def test_a_new_item_at_the_same_position_resets_the_clock(self):
        w = gate.StallWatch()
        for t in range(0, 100):
            w.update(float(t), 'Playing', f'i{t // 20}', 0.0)
        self.assertEqual(w.stalls, [])


class Refill(unittest.TestCase):
    def test_no_refill_before_the_first_change_even_with_a_short_queue(self):
        self.assertFalse(gate.needs_refill(True, 4, 0))

    def test_the_soak_refills_when_four_or_fewer_items_remain_and_the_counted_soak_never_does(self):
        self.assertEqual([gate.needs_refill(True, 18, c) for c in (0, 1, 13, 14, 15, 18)], [False, False, False, True, True, True])
        self.assertFalse(any(gate.needs_refill(False, 18, c) for c in range(0, 19)))
        self.assertEqual(gate.needs_refill(True, 36, 14), False)


class Preflight(unittest.TestCase):
    def test_standing_gpu_clients_are_recognised_by_pid_not_by_name(self):
        procs = {101: ('gnome-shell', ['libgallium.so']), 102: ('gnome-shell', ['libgallium.so']), 103: ('Xvfb', ['libvulkan_radeon.so']),
                 104: ('editor', []), 105: ('weston', []), 106: ('bramley', []), 107: ('sh', ['radeonsi_dri.so']), 108: ('weston', [])}
        real = gate.gpu_processes
        try:
            gate.gpu_processes = lambda: procs
            by_pid = gate.preflight(lambda pid: pid == 108, standing={101})
            by_name = gate.preflight()
        finally:
            gate.gpu_processes = real
        self.assertEqual(sorted(int(f.split()[0]) for f in by_pid), [102, 103, 105, 106, 107])
        self.assertEqual(sorted(int(f.split()[0]) for f in by_name), [105, 106, 107, 108])


def maps(*libs):
    return [f'7f00 r-xp 00000000 08:01 {i} /usr/lib/x86_64-linux-gnu/{lib}' for i, lib in enumerate(libs)]


class GpuClients(unittest.TestCase):
    def reported(self, comm, *libs):
        real = gate.gpu_processes
        try:
            gate.gpu_processes = lambda: {101: (comm, gate.matched_libs(maps(*libs)))}
            return [f.split(' ', 1)[1] for f in gate.preflight()]
        finally:
            gate.gpu_processes = real

    def test_a_process_mapping_only_libllvm_is_not_a_gpu_client(self):
        self.assertEqual(self.reported('postgres', 'libLLVM.so.21.1'), [])

    def test_libllvm_does_not_hide_a_mesa_client_and_only_the_driver_is_reported(self):
        self.assertEqual(self.reported('app', 'libLLVM.so.21.1', 'libgallium-25.so'), ["app ['libgallium-25.so']"])

    def test_a_process_mapping_only_radv_is_a_gpu_client(self):
        self.assertEqual(self.reported('app', 'libvulkan_radeon.so'), ["app ['libvulkan_radeon.so']"])

    def test_a_process_mapping_radeonsi_is_a_gpu_client(self):
        self.assertEqual(self.reported('app', 'radeonsi_dri.so'), ["app ['radeonsi_dri.so']"])

    def test_weston_and_bramley_are_gpu_clients_without_any_library(self):
        self.assertEqual((self.reported('weston'), self.reported('bramley')), (['weston []'], ['bramley []']))

    def test_a_process_with_no_gpu_library_and_another_name_is_not_a_gpu_client(self):
        self.assertEqual(self.reported('editor', 'libc.so.6'), [])


class Controls(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)
        os.makedirs(SCRATCH)
        self.addCleanup(shutil.rmtree, SCRATCH, True)

    def test_the_missing_artifact_control_breaks_when_an_empty_directory_passes_a_check(self):
        write_controls(SCRATCH)
        real = gate.checks
        try:
            gate.checks = lambda out, pre='': dict(real(out, pre), R6={'verdict': 'PASS', 'reasons': []})
            v = gate.verdict(SCRATCH)
        finally:
            gate.checks = real
        self.assertEqual((v['controls']['missingArtifact']['holds'], v['gate']), (False, 'FAIL'))
        try:
            gate.checks = lambda out, pre='': dict(real(out, pre), R6={'verdict': 'FAIL', 'reasons': ['bytes look wrong']})
            v = gate.verdict(SCRATCH)
        finally:
            gate.checks = real
        self.assertEqual((v['controls']['missingArtifact']['holds'], v['gate']), (False, 'FAIL'))

    def test_every_control_holds_on_evidence_that_fails_for_its_reason(self):
        write_controls(SCRATCH)
        v = gate.verdict(SCRATCH)
        self.assertEqual(({k: c['holds'] for k, c in v['controls'].items() if k != 'holds'}, v['gate']),
                         ({'noUserToken': True, 'badDeveloperToken': True, 'noStream': True, 'cutTail': True, 'leak': True, 'stall': True, 'guardKill': True,
                           'missingArtifact': True}, 'PASS'))

    def test_the_stall_control_needs_both_an_output_underflow_and_a_pacing_lag(self):
        for name, mutate, seen in (('both', lambda out: None, (True, [2.9], 1)),
                                   ('underflow only', edit('stall.stall.json', lambda d: d['after'].update(position=46.0)), (False, [], 1)),
                                   ('lag only: a dead counter', edit('stall.stall.json', lambda d: d['after'].update(outputUnderflows=0)), (False, [2.9], 0))):
            with self.subTest(name):
                write_controls(SCRATCH)
                mutate(SCRATCH)
                stall = gate.verdict(SCRATCH)['controls']['stall']
                self.assertEqual((stall['holds'], stall['lagS'], stall['outputUnderflows']), seen)

    def test_each_control_breaks_when_its_check_passes_or_fails_vacuously(self):
        for name, mutate, control in (('signed-out F2 fails', put('nouser.f2.json', banner=None), 'noUserToken'),
                                      ('signed-out F4 passes', put('nouser.f4.json', started=True, reps=[{'audioMs': 800}] * 5), 'noUserToken'),
                                      ('signed-out R1 missing', remove('nouser.shipped.r1.json'), 'noUserToken'),
                                      ('signed-out F9 passes', put('nouser.f9.json', started=True, reps=[{'audioMs': 800}] * 5), 'noUserToken'),
                                      ('bad token F3 passes', put('badtoken.f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 5), 'badDeveloperToken'),
                                      ('bad token F4 missing', remove('badtoken.f4.json'), 'badDeveloperToken'),
                                      ('cut caught by a drain', edit('cut.f6.json', lambda d: d['transitions'][2]['end'].__setitem__('drainsAcked', 1)), 'cutTail'),
                                      ('cut not skipped', edit('cut.f6.json', lambda d: d['transitions'][2]['end'].__setitem__('skippedMs', 100)), 'cutTail'),
                                      ('cut caught by the sent count, not the skipped one', edit('cut.f6.json', lambda d: [d['transitions'][2]['end'].__setitem__('skippedMs', 0),
                                                                                                          d['transitions'][2]['flip'].__setitem__('sentMs', 9000)]), 'cutTail'),
                                      ('cut mislanded', edit('cut.f6.json', lambda d: d['transitions'][2]['base'].__setitem__('position', 358.0 + 191.0 + 1.5)), 'cutTail'),
                                      ('cut never sent', edit('cut.f6.json', lambda d: d['transitions'][2].__setitem__('cut', False)), 'cutTail'),
                                      ('signed-out F4 refused for another reason', put('nouser.f4.json', reps=[{'audioMs': None, 'exit': 0, 'banner': 'Error'}] * 5), 'noUserToken'),
                                      ('signed-out F5 failed on timing, not on never starting', put('nouser.f5.json', rounds=f5_rounds(pause={'exit': 0, 'appliedMs': 30, 'effectMs': 900})), 'noUserToken'),
                                      ('bad token search failed for another reason', put('badtoken.f3.json', reps=[{'exit': 1, 'wallMs': 300, 'songs': 0, 'uiMs': None, 'error': 'timed out'}] * 5), 'badDeveloperToken'),
                                      ('bad token search slow but fine', put('badtoken.f3.json', reps=[{'exit': 0, 'wallMs': 2500, 'songs': 3, 'uiMs': 2500, 'error': 'None'}] * 5), 'badDeveloperToken'),
                                      ('bad token play refused for another reason', put('badtoken.f4.json', reps=[{'audioMs': None, 'exit': 0, 'banner': 'Error'}] * 5), 'badDeveloperToken'),
                                      ('no stream F4 failed for another reason', put('nostream.f4.json', reps=[{'audioMs': None, 'exit': 0, 'banner': 'SignInNeeded'}] * 5), 'noStream'),
                                      ('leak failed on PSS, not USS', both(edit('leak.r3.json', lambda d: [r.update(uss=148.0, pss=170.0 + (12 if r['m'] >= 29 else 0)) for r in d['samples']])), 'leak'),
                                      ('a stall the pacing rule misses', edit('stall.stall.json', lambda d: d['after'].__setitem__('position', 46.0)), 'stall'),
                                      ('a stall unseen by pacing with no underflow counter', edit('stall.stall.json', lambda d: [d['after'].update(position=46.0, outputUnderflows=None),
                                                                                                                                 d['before'].update(outputUnderflows=None)]), 'stall'),
                                      ('bad token play shows sign-in-needed', put('badtoken.f4.json', reps=[{'audioMs': None, 'exit': 0, 'banner': 'SignInNeeded'}] * 5), 'badDeveloperToken'),
                                      ('a stall across a track change', edit('stall.stall.json', lambda d: d['after'].__setitem__('itemId', '2')), 'stall'),
                                      ('stall evidence missing', remove('stall.stall.json'), 'stall'),
                                      ('guard never killed', put('guardtest.json', killed=None), 'guardKill'),
                                      ('guard killed but the app lived', put('guardtest.json', appGone=False), 'guardKill'),
                                      ('wrong sink not confirmed present', put('guardtest.json', wrongSinkPresent=False), 'guardKill'),
                                      ('guard never saw the wrong sink', put('guardtest.json', wrongSinkSeen=False), 'guardKill'),
                                      ('the default sink moved', put('guardtest.json', defaultAfter='hdmi'), 'guardKill'),
                                      ('guardtest evidence missing', remove('guardtest.json'), 'guardKill'),
                                      ('cut evidence missing', remove('cut.f6.json'), 'cutTail'),
                                      ('no stream F4 started', put('nostream.f4.json', started=True, reps=[{'audioMs': None}] * 5), 'noStream'),
                                      ('leak too small', edit('leak.r3.json', lambda d: [r.__setitem__('uss', 148.0) for r in d['samples']]), 'leak'),
                                      ('leak with too few changes', edit('leak.r3.json', lambda d: [r.update(itemId=f'i{min(int(r["m"] // 3.6), 99) if r["m"] < 5 else 1 + int((r["m"] - 5) // 7)}') for r in d['samples']]), 'leak'),
                                      ('leak binary ships clean', put('build.json', ballastFeaturesExit=0), 'leak'),
                                      ('leak evidence missing', remove('leak.r3.json'), 'leak')):
            with self.subTest(name):
                write_controls(SCRATCH)
                mutate(SCRATCH)
                v = gate.verdict(SCRATCH)
                self.assertEqual((v['controls'][control]['holds'], v['gate']), (False, 'FAIL'), v['controls'][control])


if __name__ == '__main__':
    unittest.main(argv=sys.argv[:1] + [a for a in sys.argv[1:] if a.startswith('-')])
