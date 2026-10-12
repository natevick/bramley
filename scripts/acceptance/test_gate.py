import importlib.util, os, shutil, sys, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location('gate', os.environ.get('GATE_UNDER_TEST', os.path.join(HERE, 'gate.py')))
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)
SCRATCH = os.path.join(gate.ROOT, 'target', 'tmp', f'test-gate-{os.getpid()}')
RADEONSI, LLVMPIPE, SINK, SONG = 'AMD Radeon RX 5700 XT (radeonsi, navi10)', 'llvmpipe (LLVM 21.1.8, 256 bits)', 'null-1', '6781024437'
SESSIONS = ('shipped', 'ballast', 'signedout', 'browse', 'offline', 'cpu')


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
             'banner': None, 'itemId': f'i{i // 15}', 'position': 20.0 + 10 * i, 'seeks': 0} for i in range(n)]


def played(**extra):
    return dict({'t0': 1000.0, 't1': 1050.0, 'started': True, 'song': SONG, 'reps': [{'audioMs': 800}] * 5, 'renderers': [RADEONSI],
                 'final': {'itemId': SONG, 'position': 37.5, 'duration': 355.2, 'sentMs': 57000}}, **extra)


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
        w(f'{name}.guard.json', {'pid': 7, 'killed': None, 'log': log})
    for name, r1 in (('shipped', (170.0, 148.0, 198.0)), ('ballast', (233.0, 215.0, 260.0))):
        w(f'{name}.r2.json', {'pid': 7, 'samples': rows(20, 72.0, 50.0, 98.0, renderer, playing=False)})
        w(f'{name}.r1.json', {'pid': 7, 'started': True, 'prePlay': {'default': 'iec958'}, 'ballastMiB': 67 if name == 'ballast' else None, 'samples': rows(60, *r1, renderer)})
    s = lambda name, **ev: dict(ev, session=name, renderers=[renderer])
    w('f1.json', {'starts': [{'ms': 110, 'route': 'Home', 'playlists': 34}] * 5, 'renderers': [renderer] * 5})
    w('f2.json', s('signedout', t0=1000.0, t1=1020.0, banner='SignInNeeded', playExit=5, played=False, streams=[]))
    w('f3.json', s('browse', t0=1000.0, t1=1010.0, reps=[{'exit': 0, 'wallMs': 250, 'songs': 10, 'uiMs': 420}] * 5))
    w('f4.json', s('shipped', **played()))
    w('f5.json', s('shipped', t0=1000.0, t1=1050.0, started=True, rounds=[{c: {'matched': True, 'ms': 40} for c in gate.F5_COMMANDS}] * 5))
    w('f6.json', s('shipped', t0=1000.0, t1=1050.0, started=True, transitions=[{'pass': True}] * 3))
    w('f7.json', {'mcpTotal': 34, 'largestTracks': 250, 'endReached': True, 'artTextures': 60, 'renderers': [renderer]})
    w('f8.json', s('shipped', t0=1000.0, t1=1050.0, started=True, expected=['a', 'b', 'c', 'd'], seen=['a', 'b', 'c', 'd'], exits=[0] * 4))
    w('f9.json', s('browse', **played(tools=gate.TOOLS, searchHasSong=True)))
    w('f10-badid.json', s('browse', t0=1000.0, t1=1010.0, alive=True, error=True))
    w('f10-net.json', s('offline', t0=1000.0, t1=1010.0, downError=True, downUi='error', alive=True, coversRecoveredS=4.0, searchAfter=True, uiAfter='results'))
    w('r3.json', s('shipped', t0=1000.0, t1=2810.0, started=True, pid=7, samples=soak()))
    w('r4.json', s('cpu', t0=1000.0, t1=1060.0, started=True, before='Playing', after='Playing', cpu=[4.0] * 30))
    w('r5.json', s('cpu', t0=1000.0, t1=1060.0, before='Paused', after='Paused', cpu=[0.5] * 30))
    real = {'tokenMtimeNs': 1, 'tokenSize': 300, 'keyringItems': [], 'cache': None}
    w('realstate.json', {'before': real, 'after': real})


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
    ('missing guard', remove('shipped.guard.json'), 'R2', 'FAIL', 'shipped.guard.json'),
    ('missing r1 in a void session', both(remove('shipped.r1.json'), put('session.verdict.json', sinkGone=False)), 'R1', 'FAIL', 'shipped.r1.json'),
]
FUNCTIONAL = [
    ('F1 slow median', put('f1.json', starts=[{'ms': 520, 'route': 'Home', 'playlists': 3}] * 3 + [{'ms': 100, 'route': 'Home', 'playlists': 3}] * 2), 'F1', 'FAIL', 'median 520'),
    ('F1 one slow start is fine', put('f1.json', starts=[{'ms': 900, 'route': 'Home', 'playlists': 3}] + [{'ms': 100, 'route': 'Home', 'playlists': 3}] * 4), 'F1', 'PASS', None),
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
    ('F3 slow CLI', put('f3.json', reps=[{'exit': 0, 'wallMs': 2100, 'songs': 3, 'uiMs': 2100}] * 5), 'F3', 'FAIL', 'wallMs median 2100'),
    ('F3 slow view', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 2100}] * 5), 'F3', 'FAIL', 'uiMs median 2100'),
    ('F3 view never showed', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': None}] + [{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 4), 'F3', 'FAIL', 'Search view'),
    ('F3 no songs', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 0, 'uiMs': 400}] * 5), 'F3', 'FAIL', 'did not return songs'),
    ('F3 error exit', put('f3.json', reps=[{'exit': 1, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 5), 'F3', 'FAIL', 'did not return songs'),
    ('F3 four reps', put('f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 4), 'F3', 'FAIL', 'did not return songs'),
    ('F4 never started', put('f4.json', started=False, reps=[{'audioMs': None}] * 5), 'F4', 'FAIL', 'never started'),
    ('F4 slow audio', put('f4.json', reps=[{'audioMs': 3100}] * 5), 'F4', 'FAIL', 'audio median 3100'),
    ('F4 slow median beside a fast play', put('f4.json', reps=[{'audioMs': 800}] * 2 + [{'audioMs': 3100}] * 3), 'F4', 'FAIL', 'audio median 3100'),
    ('F4 fast median beside slow plays', put('f4.json', reps=[{'audioMs': 800}] * 3 + [{'audioMs': 9000}] * 2), 'F4', 'PASS', None),
    ('F4 one missed play', put('f4.json', reps=[{'audioMs': None}] + [{'audioMs': 800}] * 4), 'F4', 'FAIL', '4 of 5'),
    ('F4 preview', edit('f4.json', lambda d: d['final'].__setitem__('duration', 30.0)), 'F4', 'FAIL', 'preview'),
    ('F4 stopped at 35', edit('f4.json', lambda d: d['final'].__setitem__('position', 35.0)), 'F4', 'FAIL', 'not past 35'),
    ('F4 nothing sent', edit('f4.json', lambda d: d['final'].__setitem__('sentMs', 35000)), 'F4', 'FAIL', 'not past 35'),
    ('F4 another song', edit('f4.json', lambda d: d['final'].__setitem__('itemId', '1')), 'F4', 'FAIL', 'not past 35'),
    ('F4 no stream once started', put('shipped.guard.json', log=[[990.0 + i / 4, []] for i in range(400)]), 'F4', 'VOID', 'no stream'),
    ('F4 stream elsewhere once started', edit('shipped.guard.json', lambda d: d['log'][60].__setitem__(1, ['hdmi'])), 'F4', 'VOID', 'no stream'),
    ('F4 stream outside its window only', put('shipped.guard.json', log=[[900.0, [SINK]], [1100.0, [SINK]]]), 'F4', 'VOID', 'no stream'),
    ('F4 never started and no stream is FAIL', both(put('f4.json', started=False), put('shipped.guard.json', log=[])), 'F4', 'FAIL', 'never started'),
    ('F4 missing guard', remove('shipped.guard.json'), 'F4', 'FAIL', 'shipped.guard.json'),
    ('F4 renderer', put('f4.json', renderers=['Some GPU (nouveau)']), 'F4', 'VOID', 'renderer'),
    ('F5 slow seek median', F5_SLOW('seek', 530, 3), 'F5', 'FAIL', 'seek: median 530'),
    ('F5 one slow seek is fine', F5_SLOW('seek', 900, 1), 'F5', 'PASS', None),
    ('F5 one slow pause fails', F5_SLOW('pause', 600, 1), 'F5', 'FAIL', 'pause: slowest 600'),
    ('F5 one slow resume fails', F5_SLOW('resume', 600, 1), 'F5', 'FAIL', 'resume: slowest 600'),
    ('F5 unmatched next', edit('f5.json', lambda d: d['rounds'][2].__setitem__('next', {'matched': False, 'ms': None})), 'F5', 'FAIL', 'next: a snapshot never matched'),
    ('F5 four rounds', edit('f5.json', lambda d: d['rounds'].pop()), 'F5', 'FAIL', '4 rounds'),
    ('F6 one transition cut', edit('f6.json', lambda d: d['transitions'][1].__setitem__('pass', False)), 'F6', 'FAIL', '2 of 3'),
    ('F6 two transitions', edit('f6.json', lambda d: d['transitions'].pop()), 'F6', 'FAIL', '2 of 3'),
    ('F7 no playlists', put('f7.json', mcpTotal=0), 'F7', 'FAIL', 'listed no playlists'),
    ('F7 short playlist', put('f7.json', largestTracks=99), 'F7', 'FAIL', '99 tracks'),
    ('F7 end not reached', put('f7.json', endReached=False), 'F7', 'FAIL', 'end'),
    ('F7 no covers', put('f7.json', artTextures=0), 'F7', 'FAIL', 'end'),
    ('F8 out of order', put('f8.json', seen=['a', 'c', 'b', 'd']), 'F8', 'FAIL', 'played'),
    ('F8 a failed add', put('f8.json', exits=[0, 0, 1, 0]), 'F8', 'FAIL', 'played'),
    ('F8 nothing expected', put('f8.json', expected=[], seen=[]), 'F8', 'FAIL', 'played'),
    ('F9 missing tool', put('f9.json', tools=gate.TOOLS[1:]), 'F9', 'FAIL', 'tools/list'),
    ('F9 search miss', put('f9.json', searchHasSong=False), 'F9', 'FAIL', 'MCP search'),
    ('F9 slow audio', put('f9.json', reps=[{'audioMs': 3001}] * 5), 'F9', 'FAIL', 'audio median'),
    ('F9 no stream', put('browse.guard.json', log=[]), 'F9', 'VOID', 'no stream'),
    ('F10 bad id crashed', put('f10-badid.json', alive=False), 'F10', 'FAIL', 'bad id'),
    ('F10 bad id silent', put('f10-badid.json', error=False), 'F10', 'FAIL', 'bad id'),
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
    ('R3 stall', soak_edit(lambda r: r.__setitem__('position', r['position'] - (3.0 if r['m'] >= 11 else 0))), 'R3', 'FAIL', 'stalled'),
    ('R3 a seek is not a stall', soak_edit(lambda r: r.update(position=r['position'] - (3.0 if r['m'] >= 11 else 0), seeks=int(r['m'] >= 11))), 'R3', 'PASS', None),
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
    ('R5 busy', put('r5.json', cpu=[1.5] * 30), 'R5', 'FAIL', 'CPU median 1.5'),
    ('R5 not paused', put('r5.json', before='Playing'), 'R5', 'FAIL', 'not Paused'),
    ('R5 is judged against its own bar', put('r5.json', cpu=[4.0] * 30), 'R5', 'FAIL', 'CPU median 4.0'),
    ('R6 unrecorded', edit('build.json', lambda d: d.pop('shippedBytes')), 'R6', 'FAIL', 'shippedBytes'),
] + [(f'{c} missing evidence', remove(f'{f}.json'), c, 'FAIL', f'{f}.json') for c, files in gate.EVIDENCE.items() for f in files] + [
    (f'{c} default sink changed', put('session.verdict.json', defaultAtEnd='hdmi'), c, 'VOID', 'default sink') for c in gate.CHECKS if c != 'R6']
BALLAST = [
    ('ballast passes the shipped check', put('build.json', ballastFeaturesExit=2), 'failsShippedCheck', 'FAIL'),
    ('ballast fails for another reason', edit('ballast.r1.json', lambda d: d['samples'][2].__setitem__('texturesUploaded', 0)), 'thresholdOnly', 'FAIL'),
    ('ballast fails only for another reason', both(every('ballast', 'uss', 150.0), every('ballast', 'rss', 199.0), edit('ballast.r1.json', lambda d: d['samples'][2].__setitem__('procs', 2))), 'thresholdOnly', 'FAIL'),
    ('ballast lands off band', every('ballast', 'uss', 225.0), 'landedWithinBand', 'FAIL'),
    ('ballast did not raise USS', put('ballast.r1.json', ballastMiB=40), 'ussRoseByBallast', 'FAIL'),
    ('ballast rise beyond its size plus kept covers', put('ballast.r1.json', ballastMiB=63), 'ussRoseByBallast', 'FAIL'),
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

    def test_the_ballast_rise_counts_the_cover_copies_the_sabotage_build_keeps(self):
        write_valid(SCRATCH)
        edit('ballast.r1.json', lambda d: d.update(ballastMiB=63, samples=[dict(r, decodedBytesKept=50 * 96 * 96 * 4) for r in d['samples']]))(SCRATCH)
        v = gate.verdict(SCRATCH)['ballast']
        self.assertEqual((v['keptMiB'], v['ussRise'], v['holds']), (1.76, 67.0, True))
        put('ballast.r1.json', ballastMiB=62)(SCRATCH)
        self.assertFalse(gate.verdict(SCRATCH)['ballast']['ussRoseByBallast'])

    def test_a_touched_real_token_or_keyring_or_a_missing_record_fails_the_gate(self):
        for name, mutate in (('mtime', edit('realstate.json', lambda d: d['after'].__setitem__('tokenMtimeNs', 2))),
                             ('keyring', edit('realstate.json', lambda d: d['after'].__setitem__('keyringItems', ['/item/1']))),
                             ('cache', edit('realstate.json', lambda d: d['after'].__setitem__('cache', {'developer-token.json': [2, 300]}))),
                             ('unread keyring', edit('realstate.json', lambda d: [d[k].__setitem__('keyringItems', None) for k in d])),
                             ('missing', remove('realstate.json'))):
            with self.subTest(name):
                write_valid(SCRATCH)
                mutate(SCRATCH)
                v = gate.verdict(SCRATCH)
                self.assertEqual((v['realStateUnchanged'], v['gate']), (False, 'FAIL'))

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
    for f in os.listdir(out):
        if f.split('.')[0] in ('f2', 'f3', 'f4', 'f5', 'f6', 'f9', 'r3', 'shipped'):
            shutil.copy(os.path.join(out, f), os.path.join(out, ('leak.' if f.startswith('r3') else 'nouser.') + f))
    for pre in ('badtoken.', 'nostream.'):
        for f in ('f3.json', 'f4.json'):
            shutil.copy(os.path.join(out, f), os.path.join(out, pre + f))
    put('nouser.f4.json', started=False, reps=[{'audioMs': None}] * 5)(out)
    put('nouser.f5.json', rounds=[])(out)
    put('nouser.f6.json', transitions=[])(out)
    put('nouser.f9.json', started=False, reps=[{'audioMs': None}] * 5)(out)
    put('nouser.shipped.r1.json', started=False, samples=[])(out)
    put('badtoken.f3.json', reps=[{'exit': 1, 'wallMs': 300, 'songs': 0, 'uiMs': None}] * 5)(out)
    put('badtoken.f4.json', started=False, reps=[{'audioMs': None}] * 5)(out)
    put('nostream.f4.json', started=False, reps=[{'audioMs': None}] * 5)(out)
    edit('leak.r3.json', lambda d: [r.__setitem__('uss', r['uss'] + 3 * int(r['itemId'][1:])) for r in d['samples']])(out)


class Controls(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)
        os.makedirs(SCRATCH)
        self.addCleanup(shutil.rmtree, SCRATCH, True)

    def test_every_control_holds_on_evidence_that_fails_for_its_reason(self):
        write_controls(SCRATCH)
        v = gate.verdict(SCRATCH)
        self.assertEqual(({k: c['holds'] for k, c in v['controls'].items() if k != 'holds'}, v['gate']),
                         ({'noUserToken': True, 'badDeveloperToken': True, 'noStream': True, 'leak': True, 'missingArtifact': True}, 'PASS'))

    def test_each_control_breaks_when_its_check_passes_or_fails_vacuously(self):
        for name, mutate, control in (('signed-out F2 fails', put('nouser.f2.json', banner=None), 'noUserToken'),
                                      ('signed-out F4 passes', put('nouser.f4.json', started=True, reps=[{'audioMs': 800}] * 5), 'noUserToken'),
                                      ('signed-out R1 missing', remove('nouser.shipped.r1.json'), 'noUserToken'),
                                      ('signed-out F9 passes', put('nouser.f9.json', started=True, reps=[{'audioMs': 800}] * 5), 'noUserToken'),
                                      ('bad token F3 passes', put('badtoken.f3.json', reps=[{'exit': 0, 'wallMs': 300, 'songs': 3, 'uiMs': 400}] * 5), 'badDeveloperToken'),
                                      ('bad token F4 missing', remove('badtoken.f4.json'), 'badDeveloperToken'),
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
