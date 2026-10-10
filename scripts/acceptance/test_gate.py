import importlib.util, os, shutil, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location('gate', os.path.join(HERE, 'gate.py'))
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)
SCRATCH = os.path.join(gate.ROOT, 'target', 'tmp', 'test-gate')
RADEONSI = 'AMD Radeon RX 5700 XT (radeonsi, navi10)'


def rows(n, pss, renderer=RADEONSI, playing=True):
    return [{'pss': pss, 'uss': pss - 22, 'rss': pss + 28, 'procs': 1, 'glRenderer': renderer, 'texturesUploaded': 50,
             'engineStarted': playing, 'state': 'Playing' if playing else 'Stopped', 'banner': None, 'position': 20.0 + i if playing else None} for i in range(n)]


def write_valid(out, renderer=RADEONSI, shipped_r1=170.0, ballast_r1=216.0):
    w = lambda name, data: gate.save(out, name, data)
    w('build.json', {'commit': 'c0ffee', 'dirty': False, 'llvmpipe': renderer != RADEONSI, 'shippedFeaturesExit': 0, 'ballastFeaturesExit': 1, 'ballastFeaturesOutput': 'FAIL: ships test-only features:\n  app/gate-sabotage'})
    w('session.verdict.json', {'defaultAtStart': 'iec958', 'defaultAtEnd': 'iec958', 'sinkGone': True})
    w('reference.json', {'sha256': gate.REFERENCE['sha256'], 'pss': 84.0, 'samples': rows(20, 84.0, playing=False)})
    w('shipped.r2.json', {'pss': 72.0, 'samples': rows(20, 72.0, renderer, playing=False)})
    w('shipped.r1.json', {'pss': shipped_r1, 'started': True, 'guardSeen': ['null'], 'samples': rows(60, shipped_r1, renderer)})
    w('ballast.r1.json', {'pss': ballast_r1, 'started': True, 'guardSeen': ['null'], 'ballastMiB': 45, 'samples': rows(60, ballast_r1, renderer)})


class Verdict(unittest.TestCase):
    def setUp(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)
        os.makedirs(SCRATCH)

    def tearDown(self):
        shutil.rmtree(SCRATCH, ignore_errors=True)

    def test_an_empty_evidence_directory_fails_every_check(self):
        v = gate.verdict(SCRATCH)
        self.assertEqual((v['R1']['verdict'], v['R2']['verdict'], v['gate'], v['ballast']['holds']), ('FAIL', 'FAIL', 'FAIL', False))

    def test_complete_evidence_under_the_bars_passes_and_the_ballast_holds(self):
        write_valid(SCRATCH)
        v = gate.verdict(SCRATCH)
        self.assertEqual((v['R1']['verdict'], v['R2']['verdict'], v['ballast']['holds'], v['gate']), ('PASS', 'PASS', True, 'PASS'))

    def test_llvmpipe_makes_both_checks_void_and_the_control_holds(self):
        write_valid(SCRATCH, renderer='llvmpipe (LLVM 20.1, 256 bits)')
        v = gate.verdict(SCRATCH)
        self.assertEqual((v['R1']['verdict'], v['R2']['verdict'], v['gate'], v['llvmpipeControl']['holds']), ('VOID', 'VOID', 'VOID', True))

    def test_missing_r1_evidence_is_fail_even_when_the_session_is_void(self):
        write_valid(SCRATCH)
        gate.save(SCRATCH, 'reference.json', {'sha256': gate.REFERENCE['sha256'], 'pss': 95.0, 'samples': rows(20, 95.0, playing=False)})
        os.remove(os.path.join(SCRATCH, 'shipped.r1.json'))
        self.assertEqual((gate.verdict(SCRATCH)['R1']['verdict'], gate.verdict(SCRATCH)['R2']['verdict']), ('FAIL', 'VOID'))

    def test_a_ballast_landing_outside_the_band_or_sized_from_elsewhere_does_not_hold(self):
        write_valid(SCRATCH, ballast_r1=230.0)
        self.assertFalse(gate.verdict(SCRATCH)['ballast']['holds'])
        write_valid(SCRATCH, shipped_r1=160.0)
        self.assertFalse(gate.verdict(SCRATCH)['ballast']['sizedFromThisRun'])

    def test_r1_over_the_bar_fails_on_the_threshold(self):
        write_valid(SCRATCH, shipped_r1=200.1)
        r1 = gate.verdict(SCRATCH)['R1']
        self.assertEqual((r1['verdict'], r1['reasons']), ('FAIL', ['PSS median 200.1 > 200.0']))


if __name__ == '__main__':
    unittest.main()
