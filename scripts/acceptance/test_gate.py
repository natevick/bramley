import importlib.util, os, shutil, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location('gate', os.path.join(HERE, 'gate.py'))
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)
SCRATCH = os.path.join(gate.ROOT, 'target', 'tmp', 'test-gate')
RADEONSI, LLVMPIPE, SINK = 'AMD Radeon RX 5700 XT (radeonsi, navi10)', 'llvmpipe (LLVM 21.1.8, 256 bits)', 'null-1'


def weston_says(renderer):
    def write(out):
        with open(os.path.join(out, 'weston.log'), 'w') as f:
            f.write(f'[16:19:16.134] GL renderer: {renderer}\n')
    return write


def rows(n, pss, uss, rss, renderer=RADEONSI, playing=True, pid=7):
    return [{'t': 1000.0 + i, 'pid': pid, 'pss': pss, 'uss': uss, 'rss': rss, 'procs': 1, 'glRenderer': renderer, 'texturesUploaded': 50,
             'engineStarted': playing, 'state': 'Playing' if playing else 'Stopped', 'banner': None, 'position': 20.0 + i if playing else None} for i in range(n)]


def write_valid(out, renderer=RADEONSI):
    w = lambda name, data: gate.save(out, name, data)
    w('build.json', {'commit': 'c0ffee', 'dirty': False, 'llvmpipe': renderer != RADEONSI, 'shippedFeaturesExit': 0, 'ballastFeaturesExit': 1, 'ballastFeaturesOutput': 'FAIL: ships test-only features:\n  app/gate-sabotage'})
    w('session.verdict.json', {'sink': SINK, 'defaultAtStart': 'iec958', 'defaultAtEnd': 'iec958', 'sinkGone': True})
    weston_says(RADEONSI)(out)
    for ref in gate.REFERENCE_RUNS:
        w(f'{ref}.json', {'sha256': gate.REFERENCE['sha256'], 'pid': 7, 'samples': rows(20, 84.0, 62.0, 104.0, playing=False)})
    log = [[990.0 + i / 4, [SINK]] for i in range(400)]
    for name, r1 in (('shipped', (170.0, 148.0, 198.0)), ('ballast', (233.0, 215.0, 260.0))):
        w(f'{name}.r2.json', {'pid': 7, 'samples': rows(20, 72.0, 50.0, 98.0, renderer, playing=False)})
        w(f'{name}.r1.json', {'pid': 7, 'started': True, 'prePlay': {'default': 'iec958'}, 'ballastMiB': 67 if name == 'ballast' else None, 'samples': rows(60, *r1, renderer)})
        w(f'{name}.guard.json', {'pid': 7, 'killed': None, 'log': log})
    real = {'tokenMtimeNs': 1, 'tokenSize': 300, 'keyringItems': [], 'cache': None}
    w('realstate.json', {'before': real, 'after': real})


def edit(name, fn):
    def apply(out):
        data = gate.load(out, name)
        fn(data)
        gate.save(out, name, data)
    return apply


def every(name, key, value, kind='r1'):
    return edit(f'{name}.{kind}.json', lambda d: [r.__setitem__(key, value) for r in d['samples']])


def remove(name):
    return lambda out: os.remove(os.path.join(out, name))


def both(*fns):
    return lambda out: [fn(out) for fn in fns]


DRIFTED = edit('reference-after.json', lambda d: [r.__setitem__('pss', 76.0) for r in d['samples']])
ZONE3 = both(every('shipped', 'rss', 205.0), every('shipped', 'pss', 180.0))
CASES = [
    ('R2 engine started', edit('shipped.r2.json', lambda d: d['samples'][3].__setitem__('engineStarted', True)), 'R2', 'FAIL', 'engine started'),
    ('R2 USS over the bar', every('shipped', 'uss', 101.0, 'r2'), 'R2', 'FAIL', 'USS median 101.0'),
    ('fewer than 50 covers', edit('shipped.r1.json', lambda d: d['samples'][5].__setitem__('texturesUploaded', 49)), 'R1', 'FAIL', 'covers'),
    ('two processes', edit('shipped.r1.json', lambda d: d['samples'][5].__setitem__('procs', 2)), 'R1', 'FAIL', 'exactly 1 process'),
    ('too few samples', edit('shipped.r1.json', lambda d: d['samples'].pop()), 'R1', 'FAIL', '59 samples'),
    ('never started', edit('shipped.r1.json', lambda d: d.__setitem__('started', False)), 'R1', 'FAIL', 'never started'),
    ('position stuck', every('shipped', 'position', 20.0), 'R1', 'FAIL', 'advanced 0.0'),
    ('paused in a sample', edit('shipped.r1.json', lambda d: d['samples'][9].__setitem__('state', 'Paused')), 'R1', 'FAIL', 'not Playing'),
    ('banner in a sample', edit('shipped.r1.json', lambda d: d['samples'][9].__setitem__('banner', 'SignInNeeded')), 'R1', 'FAIL', 'not Playing'),
    ('guard kill voids R1', edit('shipped.guard.json', lambda d: d.__setitem__('killed', 'linked to spdif')), 'R1', 'VOID', 'guard killed'),
    ('guard kill voids R2', edit('shipped.guard.json', lambda d: d.__setitem__('killed', 'linked to spdif')), 'R2', 'VOID', 'guard killed'),
    ('no guard polls in the window', edit('shipped.guard.json', lambda d: d.__setitem__('log', [])), 'R1', 'VOID', 'polls in the R1 window <'),
    ('link drops mid-window', edit('shipped.guard.json', lambda d: d['log'][100].__setitem__(1, [])), 'R1', 'VOID', 'not linked'),
    ('reference sha', edit('reference.json', lambda d: d.__setitem__('sha256', 'feed')), 'R1', 'VOID', 'sha'),
    ('reference renderer', edit('reference-after.json', lambda d: d['samples'][0].__setitem__('glRenderer', LLVMPIPE)), 'R2', 'VOID', 'reference-after renderer'),
    ('reference pid', edit('reference.json', lambda d: d['samples'][0].__setitem__('pid', 8)), 'R2', 'VOID', 'reference snapshot'),
    ('zone 3 drift after', both(ZONE3, DRIFTED), 'R1', 'VOID', 'drifted -7.3'),
    ('zone 2 drift reported only', DRIFTED, 'R1', 'PASS', None),
    ('zone 3 PSS over', both(ZONE3, every('shipped', 'pss', 201.0)), 'R1', 'FAIL', 'PSS median 201.0'),
    ('default sink changed', edit('session.verdict.json', lambda d: d.__setitem__('defaultAtEnd', 'hdmi')), 'R1', 'VOID', 'default sink'),
    ('wpctl failed', edit('session.verdict.json', lambda d: d.update(defaultAtStart=None, defaultAtEnd=None)), 'R2', 'VOID', 'default sink'),
    ('default restored', edit('session.verdict.json', lambda d: d.__setitem__('defaultRestored', True)), 'R1', 'VOID', 'default sink'),
    ('wpctl failed before play', edit('shipped.r1.json', lambda d: d['prePlay'].__setitem__('default', None)), 'R1', 'VOID', 'unreadable before play'),
    ('null sink left', edit('session.verdict.json', lambda d: d.__setitem__('sinkGone', False)), 'R1', 'VOID', 'left behind'),
    ('weston on llvmpipe', weston_says(LLVMPIPE), 'R2', 'VOID', 'weston renderer'),
    ('app renderer not radeonsi', every('shipped', 'glRenderer', 'Some GPU (nouveau)'), 'R1', 'VOID', 'shipped renderer'),
    ('shipped features', edit('build.json', lambda d: d.__setitem__('shippedFeaturesExit', 1)), 'R2', 'FAIL', 'check-shipped-features'),
    ('dirty tree', edit('build.json', lambda d: d.__setitem__('dirty', True)), 'R1', 'FAIL', 'dirty'),
    ('dirty unknown', edit('build.json', lambda d: d.pop('dirty')), 'R2', 'FAIL', 'dirty'),
    ('snapshot from another pid', edit('shipped.r1.json', lambda d: d['samples'][4].__setitem__('pid', 8)), 'R1', 'FAIL', 'another process'),
    ('stored medians ignored', both(edit('shipped.r1.json', lambda d: d.update(pss=1.0, uss=1.0, rss=1.0)), every('shipped', 'uss', 250.0)), 'R1', 'FAIL', 'USS median 250.0'),
    ('missing build beats void', both(remove('build.json'), edit('session.verdict.json', lambda d: d.__setitem__('sinkGone', False))), 'R1', 'FAIL', 'build.json'),
    ('missing session', remove('session.verdict.json'), 'R2', 'FAIL', 'session.verdict.json'),
    ('missing reference after', remove('reference-after.json'), 'R1', 'FAIL', 'reference-after.json'),
    ('missing weston log', remove('weston.log'), 'R1', 'FAIL', 'weston.log'),
    ('missing guard', remove('shipped.guard.json'), 'R2', 'FAIL', 'shipped.guard.json'),
    ('missing r1 in a void session', both(remove('shipped.r1.json'), edit('session.verdict.json', lambda d: d.__setitem__('sinkGone', False))), 'R1', 'FAIL', 'shipped.r1.json'),
]
BALLAST = [
    ('ballast passes the shipped check', edit('build.json', lambda d: d.__setitem__('ballastFeaturesExit', 2)), 'failsShippedCheck', 'FAIL'),
    ('ballast fails for another reason', edit('ballast.r1.json', lambda d: d['samples'][2].__setitem__('texturesUploaded', 0)), 'thresholdOnly', 'FAIL'),
    ('ballast lands off band', every('ballast', 'uss', 225.0), 'landedWithinBand', 'FAIL'),
    ('ballast did not raise USS', edit('ballast.r1.json', lambda d: d.__setitem__('ballastMiB', 40)), 'ussRoseByBallast', 'FAIL'),
    ('ballast-only void', edit('ballast.guard.json', lambda d: d.__setitem__('killed', 'linked to spdif')), 'thresholdOnly', 'VOID'),
]


class Verdict(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)
        os.makedirs(SCRATCH)
        self.addCleanup(shutil.rmtree, SCRATCH, True)

    def test_an_empty_evidence_directory_fails_every_check(self):
        v = gate.verdict(SCRATCH)
        self.assertEqual((v['R1']['verdict'], v['R2']['verdict'], v['gate'], v['ballast']['holds']), ('FAIL', 'FAIL', 'FAIL', False))

    def test_complete_evidence_passes_in_zone_2_and_the_ballast_holds_in_zone_1(self):
        write_valid(SCRATCH)
        v = gate.verdict(SCRATCH)
        self.assertEqual((v['R1']['verdict'], v['R1']['zone'], v['R2']['verdict'], v['ballast']['zone'], v['ballast']['holds'], v['gate']), ('PASS', 2, 'PASS', 1, True, 'PASS'))

    def test_llvmpipe_voids_both_checks_and_the_control_needs_llvmpipe_seen(self):
        write_valid(SCRATCH, renderer=LLVMPIPE)
        v = gate.verdict(SCRATCH)
        self.assertEqual((v['R1']['verdict'], v['R2']['verdict'], v['gate'], v['llvmpipeControl']['holds']), ('VOID', 'VOID', 'VOID', True))
        write_valid(SCRATCH, renderer='Some GPU (nouveau)')
        gate.save(SCRATCH, 'build.json', dict(gate.load(SCRATCH, 'build.json'), llvmpipe=True))
        self.assertFalse(gate.verdict(SCRATCH)['llvmpipeControl']['holds'])

    def test_each_rule_moves_its_check_with_its_own_reason(self):
        for name, mutate, check, want, reason in CASES:
            with self.subTest(name):
                write_valid(SCRATCH)
                mutate(SCRATCH)
                got = gate.verdict(SCRATCH)[check]
                self.assertEqual(got['verdict'], want, got['reasons'])
                self.assertTrue(reason is None or any(reason in r for r in got['reasons']), got['reasons'])

    def test_each_ballast_clause_breaks_the_control_and_the_gate(self):
        for name, mutate, clause, gate_verdict in BALLAST:
            with self.subTest(name):
                write_valid(SCRATCH)
                mutate(SCRATCH)
                v = gate.verdict(SCRATCH)
                self.assertEqual((v['ballast'][clause], v['ballast']['holds'], v['gate']), (False, False, gate_verdict))


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


class Isolation(unittest.TestCase):
    def test_a_launch_against_the_real_config_or_keyring_is_refused(self):
        tmp = os.path.join(SCRATCH, 'config')
        cache = os.path.join(tmp, 'cache')
        for env in ({}, {'BRAMLEY_CONFIG_DIR': gate.cs.REAL_CONFIG, 'BRAMLEY_KEYRING_SERVICE': 'bramley-gate-x', 'XDG_CACHE_HOME': cache},
                    {'BRAMLEY_CONFIG_DIR': tmp, 'XDG_CACHE_HOME': cache}, {'BRAMLEY_CONFIG_DIR': tmp, 'BRAMLEY_KEYRING_SERVICE': 'bramley', 'XDG_CACHE_HOME': cache},
                    {'BRAMLEY_CONFIG_DIR': tmp, 'BRAMLEY_KEYRING_SERVICE': 'bramley-gate-x'},
                    {'BRAMLEY_CONFIG_DIR': tmp, 'BRAMLEY_KEYRING_SERVICE': 'bramley-gate-x', 'XDG_CACHE_HOME': os.path.dirname(gate.cs.REAL_CACHE)}):
            with self.subTest(env):
                self.assertRaises(RuntimeError, gate.cs.assert_isolated, env)
        gate.cs.assert_isolated({'BRAMLEY_CONFIG_DIR': tmp, 'BRAMLEY_KEYRING_SERVICE': 'bramley-gate-x', 'XDG_CACHE_HOME': cache})

if __name__ == '__main__':
    unittest.main()
