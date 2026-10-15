import functools, importlib.util, json, os, shutil, sys, tempfile, time, unittest
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, f'{name}.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


drive, probe_seek = load('drive'), load('probe_seek')
gate = drive.gate
TMP = os.path.join(gate.ROOT, 'target', 'tmp')


class Player:
    def __init__(self, out, log):
        self.out, self.log, self.name, self.pid, self.calls, self.last_t = out, log, 'soak.shipped', os.getpid(), [], None
        self.queue, self.item, self.base, self.anchor, self.seq, self.applied, self.skipped, self.pending = [], None, 0.0, time.time(), 0, 0, 0, None

    def position(self):
        return self.base + time.time() - self.anchor if self.item else 0.0

    def snap(self):
        if self.pending and time.time() >= self.pending[0]:
            _, self.applied, self.base, self.anchor, self.skipped = self.pending[0], self.pending[1], self.pending[2], time.time(), self.skipped + 1000
            self.log.write(f'JS[log] TRACE {time.time() * 1000:.3f} set HTMLAudioElement#2 currentTime {self.base}\nJS[log] TRACE {time.time() * 1000:.3f} mark step seek done\n')
            self.log.flush()
            self.pending = None
        if self.item and self.position() >= 30.0 and self.queue:
            self.item, self.base, self.anchor = self.queue.pop(0), 0.0, time.time()
        return {'pid': self.pid, 'state': 'Playing', 'itemId': self.item, 'position': self.position(), 'duration': 30.0, 'applied': self.applied, 'seq': self.seq,
                'sentMs': round(self.position() * 1000), 'skippedMs': self.skipped, 'outputUnderflows': 0, 'texturesUploaded': 50, 'banner': None, 'queueLength': 99}

    def cli(self, *args):
        self.calls.append(args)
        self.last_t = time.time()
        if args[0] == 'now':
            return 0, self.snap(), 0
        self.seq += 1
        if args[0] == 'play':
            self.item, self.base, self.anchor, self.applied = args[1], 0.0, time.time(), self.seq
            self.log.write(f'JS[log] TRACE {time.time() * 1000:.3f} set HTMLAudioElement#2 currentTime 0\n')
            self.log.flush()
        elif args[0] == 'queue':
            self.queue.append(args[2])
            self.applied = self.seq
        elif args[0] == 'seek':
            self.log.write(f'JS[log] TRACE {time.time() * 1000:.3f} mark step seek start\n')
            self.log.flush()
            self.pending = (time.time() + 0.3, self.seq, float(args[1]), 0, 0)
        return 0, {'seq': self.seq, 'engine': 1}, 0

    def until(self, pred, timeout, step=0.01):
        end = time.time() + timeout
        while time.time() < end:
            s = self.snap()
            if pred(s):
                return s, time.time()
            time.sleep(step)
        return self.snap(), None

    def safe_to_play(self):
        pass

    def renderers(self):
        return ['radeonsi']


def soak(trace, installed=True):
    os.makedirs(TMP, exist_ok=True)
    out = tempfile.mkdtemp(dir=TMP)
    log = open(os.path.join(out, 'soak.shipped.log'), 'w+')
    log.write('JS[log] TRACE 1.000 trace installed\n' if installed else 'engine up\n')
    log.flush()
    a = Player(out, log)
    windowed = functools.partial(probe_seek.windowed_seek, post=1.0, poll=0.05)
    patches = (mock.patch.object(drive, 'SOAK_S', 3), mock.patch.object(drive, 'SOAK_EVERY_S', 1), mock.patch.object(drive, 'SOAK_TRACK_S', 1.0), mock.patch.object(drive, 'TRACE_RUN', trace),
               mock.patch.object(drive, 'PACER', gate.Pacer()), mock.patch.object(drive.probe_seek, 'windowed_seek', windowed))
    with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
        drive.r3(a, 'soak.', loop=True)
    log.close()
    return a, json.load(open(os.path.join(out, 'soak.r3.json'))), out


class SoakTrace(unittest.TestCase):
    def tearDown(self):
        [shutil.rmtree(d, ignore_errors=True) for d in getattr(self, 'dirs', [])]

    def run_soak(self, trace):
        a, ev, out = soak(trace)
        self.dirs = getattr(self, 'dirs', []) + [out]
        return a, ev

    def test_a_traced_soak_records_a_window_per_seek_and_changes_nothing_else(self):
        plain, plain_ev = self.run_soak(False)
        traced, ev = self.run_soak(True)
        seeks = [c for c in traced.calls if c[0] == 'seek']
        self.assertEqual((len(seeks), len(ev['seekWindows']), ev['traceInstalled']), (1, 1, True))
        self.assertEqual(([c[0] for c in traced.calls if c[0] != 'now'], len(ev['seekTimes'])), ([c[0] for c in plain.calls if c[0] != 'now'], len(plain_ev['seekTimes'])))
        self.assertEqual([c for c in plain.calls if c[0] == 'seek'], seeks)
        self.assertNotIn('seekWindows', plain_ev)

    def test_each_window_has_its_polls_the_settle_and_the_trace_between_the_marks(self):
        _, ev = self.run_soak(True)
        window = ev['seekWindows'][0]
        got = gate.probe_seek_derive(window['rows'], window['tSeek'], window['seq'])
        self.assertTrue(got['polls'] >= 15 and 0.2 <= got['tApp'] <= 0.6 and got['tSkip'] is not None, got)
        self.assertEqual([k for _, k, _ in window['trace']], ['seekToTime', 'currentTime', 'seekToTime'])
        self.assertNotIn(window['rows'][0]['itemId'], drive.ALBUM)
        self.assertEqual(gate.soak_trace_rules(ev)['verdict'], 'NOT CAPTURED')

    def test_a_log_without_the_trace_installed_is_recorded_and_voids_the_trace_verdict(self):
        a, ev, out = soak(True, installed=False)
        self.dirs = [out]
        self.assertEqual((ev['traceInstalled'], gate.soak_trace_rules(ev)['verdict']), (False, 'VOID'))

    def test_a_window_does_not_block_the_caller(self):
        a = Player(TMP, open(os.devnull, 'w'))
        a.item = 'x'
        log = mock.Mock(mark=lambda: 0, lines=lambda s, e: [])
        windows, begun = [], time.time()
        watcher = probe_seek.windowed_seek(a, log, ('seek', '5'), windows, post=1.0, poll=0.05)
        self.assertTrue(time.time() - begun < 0.5 and windows == [])
        watcher.join(5)
        self.assertEqual((len(windows), windows[0]['seq']), (1, 1))


if __name__ == '__main__':
    unittest.main(argv=sys.argv[:1] + [a for a in sys.argv[1:] if a.startswith('-')])
