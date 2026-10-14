import contextlib, importlib.util, json, os, sys, tempfile, types, unittest
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, f'{name}.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gate, probe_seek = load('gate'), load('probe_seek')
IDS, DURATIONS, TITLE = {'LONG': 'i.long1', 'SHORT': 'i.short1'}, {'i.long1': 500.79, 'i.short1': 120.0, 'i.pred1': 139.88}, 'Some Title'
IDS2 = dict(IDS, PRED='i.pred1')
ALBUM = ['i.zero', 'i.pred1', 'i.long1', 'i.next1', 'i.next2']


class World:
    def __init__(self, slow_if, long_duration=500.79, snap_cost=0.0, slow_by=None, pred_duration=139.88, short_duration=120.0, installed=True):
        self.t, self.last, self.slow_if, self.snap_cost, self.slow_by = 1000.0, 1000.0, slow_if, snap_cost, slow_by
        self.queue, self.natural, self.lines, self.adds, self.installed = [], False, [], 0, installed
        self.item, self.state, self.pos, self.pending = None, 'Stopped', 0.0, None
        self.seq = self.applied = self.sent = self.skipped = 0
        self.last_t, self.durations = None, dict(DURATIONS, **{'i.long1': long_duration, 'i.pred1': pred_duration, 'i.short1': short_duration})
        self.pacer = gate.Pacer()

    def sleep(self, dt):
        self.t += dt

    def clock(self):
        return self.t

    def sync(self):
        while self.last < self.t:
            end = min(self.t, self.pending['land']) if self.pending else self.t
            if self.state == 'Playing':
                self.pos += end - self.last
                self.sent += (end - self.last) * 1000
                if self.pos >= self.durations.get(self.item, 200.0):
                    self.pos, self.natural = 0.0, bool(self.queue)
                    if self.queue:
                        self.item = self.queue.pop(0)
                    else:
                        self.state = 'Stopped'
            self.last = end
            if self.pending and end >= self.pending['land']:
                p, self.pending = self.pending, None
                self.applied = p['seq']
                if p['kind'] == 'play':
                    self.state, self.pos = 'Playing', 0.0
                else:
                    self.pos, self.skipped = p['target'], self.skipped + 1000
                    self.log(end, 'set HTMLAudioElement#2 currentTime %s' % p['target'])
                    self.log(end, 'mark step seek done')

    def log(self, at, text):
        self.lines.append('JS[log] TRACE %.3f %s' % ((at - 1000.0) * 1000, text))

    def snap(self):
        self.sync()
        self.t += self.snap_cost
        shown = self.pending['target'] if self.pending and self.pending['kind'] == 'seek' else self.pos
        return {'itemId': self.item, 'state': self.state, 'position': shown, 'duration': self.durations.get(self.item, 200.0) if self.item else None, 'applied': self.applied, 'seq': self.seq,
                'sentMs': round(self.sent), 'skippedMs': round(self.skipped)}

    def cli(self, *args):
        self.sync()
        kind = gate.license_kind(args)
        if kind:
            self.pacer.take(kind)
        self.last_t = self.t
        if args[0] == 'now':
            return 0, {'title': TITLE}, 0
        self.seq += 1
        if args[0] == 'queue':
            self.queue.append(args[2])
            self.adds += 1
            self.applied = self.seq
            return 0, {'seq': self.seq, 'engine': 1}, 0
        target = float(args[1]) if args[0] == 'seek' else None
        if args[0] == 'play':
            self.item, self.state, self.queue, self.natural = args[1], 'Loading', [], False
            if self.installed and not self.lines:
                self.log(self.t, 'trace installed')
        else:
            self.log(self.t + 0.01, 'mark step seek start')
        delay = 0.3 if args[0] == 'play' else self.slow_by(self, target) if self.slow_by else self.slow_if(self.pos, target)
        self.pending = {'kind': args[0], 'seq': self.seq, 'target': target, 'land': self.t + delay}
        return 0, {'seq': self.seq, 'engine': 1}, 0


class Log:
    def __init__(self, world):
        self.world = world

    def mark(self):
        return len(self.world.lines)

    def lines(self, start, end):
        return self.world.lines[start:end]


def run(slow_if, ids=IDS, **world):
    w, noted = World(slow_if, **world), []
    d = types.SimpleNamespace(gate=gate, PACER=w.pacer, ALBUM=ALBUM, note_names=lambda found, kind: noted.append(found))
    ev = {'arms': {}}
    try:
        if 'PRED' in ids:
            probe_seek.sequence2(d, w, ids, ev, w.clock, w.sleep, Log(w))
        else:
            probe_seek.sequence(d, w, ids, ev, w.clock, w.sleep)
    finally:
        run.world, run.noted, run.ev = w, noted, ev
    return ev


defect = lambda pos, target: 4.5 if pos < 200 and target > 400 else 0.3
fast = lambda pos, target: 0.3
slow_everywhere = lambda pos, target: 4.5 if target > 400 else 0.3


class Sequence(unittest.TestCase):
    def test_a_defect_shaped_player_is_read_as_the_defect_with_five_license_actions(self):
        verdict = gate.probe_seek_rules(run(defect))
        self.assertEqual((verdict['verdict'], verdict['defect'], verdict['originEffect']), ('PASS', True, 'supports'), verdict['reasons'])
        a, b, c = (verdict['arms'][k] for k in 'ABC')
        self.assertEqual((a['tSkip'], a['tApp'], a['itemId'], a['seqDelta'], c['itemId']), (4.6, 4.6, 'LONG', 1, 'SHORT'))
        self.assertTrue(150 <= a['origin'] <= 155 and 295 <= b['origin'] <= 300, (a['origin'], b['origin']))
        self.assertEqual(([k for _, k in run.world.pacer.actions], 'replayed' in run.ev), (['play', 'seek-to-end', 'seek-to-end', 'play', 'seek-to-end'], False))
        self.assertEqual([x['polls'] for x in (a, b, c)], [71, 71, 71])
        self.assertEqual([run.ev['arms'][k]['target'] for k in 'ABC'], [490.8, 490.8, 110.0])

    def test_the_record_carries_labels_and_never_an_id_or_a_name(self):
        body = json.dumps(run(defect))
        self.assertEqual([x in body for x in (*IDS.values(), TITLE)], [False] * 3)
        self.assertEqual(len(run.noted), 2)

    def test_a_player_that_lands_fast_is_inconclusive_never_a_pass_and_replays_the_ended_item_within_six_actions(self):
        verdict = gate.probe_seek_rules(run(fast))
        self.assertEqual((verdict['verdict'], run.ev['replayed'], len(run.world.pacer.actions)), ('INCONCLUSIVE', True, 6), verdict['reasons'])
        self.assertEqual((verdict['arms']['B']['origin'] > 290, verdict['arms']['B']['tApp'] <= 0.5), (True, True))

    def test_a_player_slow_from_every_origin_refutes_the_origin_effect(self):
        verdict = gate.probe_seek_rules(run(slow_everywhere))
        self.assertEqual((verdict['verdict'], verdict['originEffect']), ('REFUTED', 'refutes'))

    def test_a_slow_status_read_cannot_resolve_the_arms_and_is_void(self):
        verdict = gate.probe_seek_rules(run(defect, snap_cost=0.4))
        self.assertEqual(verdict['verdict'], 'VOID')

    def test_the_wrong_long_item_is_refused_after_one_license_action(self):
        with self.assertRaisesRegex(RuntimeError, 'not the expected item'):
            run(defect, long_duration=400.0)
        self.assertEqual((len(run.world.pacer.actions), run.ev['arms']), (1, {}))

    def test_a_seventh_license_action_is_refused_before_it_is_taken(self):
        w = World(defect)
        w.pacer.note('earlier', 2)
        d = types.SimpleNamespace(gate=gate, PACER=w.pacer, note_names=lambda *a: None)
        with self.assertRaisesRegex(RuntimeError, 'refusing'):
            probe_seek.sequence(d, w, IDS, {'arms': {}}, w.clock, w.sleep)
        self.assertEqual(len(w.pacer.actions), 6)

    def test_item_ids_must_be_two_different_plain_ids(self):
        good = {'BRAMLEY_PROBE_LONG': 'i.abc-1', 'BRAMLEY_PROBE_SHORT': '6781024437'}
        self.assertEqual(probe_seek.probe_ids(good), {'LONG': 'i.abc-1', 'SHORT': '6781024437'})
        for bad in ({}, dict(good, BRAMLEY_PROBE_SHORT='a b'), dict(good, BRAMLEY_PROBE_LONG='a;b'), dict(good, BRAMLEY_PROBE_SHORT='i.abc-1')):
            with self.assertRaises(RuntimeError):
                probe_seek.probe_ids(bad)


entry_slow = lambda w, target: 4.5 if w.natural and w.item == 'i.long1' and target > 400 else 0.3
always_slow = lambda w, target: 4.5 if target > 400 else 0.3


class Sequence2(unittest.TestCase):
    def test_a_player_slow_only_after_a_natural_entry_passes_and_records_the_entry_and_the_trace(self):
        verdict = gate.probe2_rules(run(None, IDS2, slow_by=entry_slow))
        self.assertEqual((verdict['verdict'], verdict['entryEffect']), ('PASS', 'supports'), verdict['reasons'])
        ev = run.ev
        self.assertEqual((ev['entry']['from'], ev['entry']['item'], ev['entry']['seqAtEntry'] == ev['entry']['lastSeq']), ('PRED', 'LONG', True))
        self.assertEqual(([x['polls'] for x in verdict['arms'].values()], run.world.adds), ([71, 71, 71], 3))
        self.assertEqual([[k for _, k, _ in ev['arms'][a]['trace']] for a in 'AFC'], [['seekToTime', 'currentTime', 'seekToTime']] * 3)
        self.assertEqual([ev['arms'][k]['target'] for k in 'AFC'], [490.8, 490.8, 110.0])
        kinds = [k for _, k in run.world.pacer.actions]
        self.assertTrue(len(kinds) <= gate.PROBE2_ACTIONS and kinds[:3] == ['play', 'item-change', 'seek-to-end'], kinds)

    def test_a_player_that_lands_fast_everywhere_does_not_reproduce(self):
        verdict = gate.probe2_rules(run(None, IDS2, slow_by=lambda w, target: 0.3))
        self.assertEqual((verdict['verdict'], verdict['defect']), ('INCONCLUSIVE', False))
        self.assertEqual([k for _, k in run.world.pacer.actions].count('item-change'), 2)

    def test_a_player_slow_after_a_fresh_entry_too_refutes_the_entry_effect(self):
        verdict = gate.probe2_rules(run(None, IDS2, slow_by=always_slow))
        self.assertEqual((verdict['verdict'], verdict['entryEffect']), ('REFUTED', 'refutes'))

    def test_the_record_carries_labels_and_never_an_id_or_a_name(self):
        body = json.dumps(run(None, IDS2, slow_by=entry_slow))
        self.assertEqual([x in body for x in (*IDS2.values(), TITLE)], [False] * 4)

    def test_a_wrong_pred_is_refused_after_one_action_and_unrelated_ids_before_any(self):
        with self.assertRaisesRegex(RuntimeError, 'not the expected item'):
            run(None, IDS2, slow_by=entry_slow, pred_duration=100.0)
        self.assertEqual(len(run.world.pacer.actions), 1)
        with self.assertRaisesRegex(RuntimeError, 'did not install the trace'):
            run(None, IDS2, slow_by=entry_slow, installed=False)
        self.assertEqual(len(run.world.pacer.actions), 1)
        with self.assertRaisesRegex(RuntimeError, 'consecutive'):
            run(None, dict(IDS2, PRED='i.zero'), slow_by=entry_slow)
        self.assertEqual(len(run.world.pacer.actions), 0)
        for kw, why in ((dict(long_duration=400.0), 'unexpected duration'), (dict(short_duration=400.0), 'not the expected item')):
            with self.assertRaisesRegex(RuntimeError, why):
                run(None, IDS2, slow_by=entry_slow, **kw)

    def test_the_cap_refuses_an_eleventh_action(self):
        w = World(None, slow_by=entry_slow)
        w.pacer.note('earlier', 8)
        d = types.SimpleNamespace(gate=gate, PACER=w.pacer, ALBUM=ALBUM, note_names=lambda *a: None)
        with self.assertRaisesRegex(RuntimeError, 'refusing'):
            probe_seek.sequence2(d, w, IDS2, {'arms': {}}, w.clock, w.sleep, Log(w))
        self.assertEqual(len(w.pacer.actions), 10)

    def test_trace_lines_become_a_timeline_of_what_the_seek_waited_on(self):
        lines = ['JS[log] TRACE 100.000 mark step seek start', 'JS[log] TRACE 100.500 call fetch#1 fetch GET cdn.example~ab12',
                 'JS[log] TRACE 101.000 resolve fetch#1 200 Response#3 cache-control=str9 content-length=5000 vary=str3', 'JS[log] TRACE 101.250 call SourceBuffer#1 appendBuffer ab4096',
                 'JS[log] TRACE 101.300 throw SourceBuffer#1 appendBuffer QuotaExceededError', 'JS[log] TRACE 102.000 call SourceBuffer#1 remove 0,12',
                 'JS[log] TRACE 103.000 set HTMLAudioElement#2 currentTime 490.8', 'JS[log] TRACE 103.500 get HTMLMediaElement#1 currentTime 3',
                 'TRACE 103.100 cdm-event keys=1 {"type":"keys","keys":2}', 'not a trace line', 'JS[log] TRACE 104.000 mark step seek done']
        self.assertEqual(probe_seek.parse_trace(lines), [[0.0, 'seekToTime', 'start'], [0.5, 'fetch', 'GET cdn.example~ab12'], [1.0, 'fetch', '200 content-length=5000'],
                                                         [1.2, 'append', 'ab4096'], [1.3, 'appendThrow', 'QuotaExceededError'], [2.0, 'remove', '0,12'], [3.0, 'currentTime', '490.8'],
                                                         [3.1, 'cdm', 'keys'], [4.0, 'seekToTime', 'done']])

    def test_a_binary_without_the_trace_feature_is_refused_before_launch_and_one_with_it_gets_the_log(self):
        tmp = os.path.join(gate.ROOT, 'target', 'tmp')
        os.makedirs(tmp, exist_ok=True)
        with tempfile.NamedTemporaryFile('w+', dir=tmp) as log, tempfile.NamedTemporaryFile('wb', dir=tmp) as binary:
            fake = types.SimpleNamespace(log=log, until=lambda *a: None)
            d = types.SimpleNamespace(gate=gate, PACER=gate.Pacer(), evidence=lambda *a: None, app=lambda *a: contextlib.nullcontext(fake))
            env = {'BRAMLEY_PROBE_PRED': 'a', 'BRAMLEY_PROBE_LONG': 'b', 'BRAMLEY_PROBE_SHORT': 'c'}
            binary.write(b'\x7fELF plain build')
            binary.flush()
            with mock.patch.dict(os.environ, env), mock.patch.object(probe_seek.time, 'sleep'), mock.patch.object(probe_seek, 'sequence2') as sequence2:
                with self.assertRaisesRegex(RuntimeError, 'not a trace build'):
                    probe_seek.session2(d, '.', binary.name, {}, 'sink')
                binary.write(b'\x00bramley-unshipped-feature:engine/trace\x00')
                binary.flush()
                probe_seek.session2(d, '.', binary.name, {}, 'sink')
            self.assertEqual(sequence2.call_args.kwargs['trace'].path, log.name)

    def test_three_different_plain_ids_are_required(self):
        env = {'BRAMLEY_PROBE_PRED': 'a', 'BRAMLEY_PROBE_LONG': 'b', 'BRAMLEY_PROBE_SHORT': 'c'}
        self.assertEqual(probe_seek.probe_ids(env, ('PRED', 'LONG', 'SHORT')), {'PRED': 'a', 'LONG': 'b', 'SHORT': 'c'})
        for bad in (dict(env, BRAMLEY_PROBE_PRED='b'), dict(env, BRAMLEY_PROBE_SHORT='x y'), {k: v for k, v in env.items() if k != 'BRAMLEY_PROBE_LONG'}):
            with self.assertRaises(RuntimeError):
                probe_seek.probe_ids(bad, ('PRED', 'LONG', 'SHORT'))


if __name__ == '__main__':
    unittest.main(argv=sys.argv[:1] + [a for a in sys.argv[1:] if a.startswith('-')])
