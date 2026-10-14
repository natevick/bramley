import importlib.util, json, os, sys, types, unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, f'{name}.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gate, probe_seek = load('gate'), load('probe_seek')
IDS, DURATIONS, TITLE = {'LONG': 'i.long1', 'SHORT': 'i.short1'}, {'i.long1': 500.79, 'i.short1': 120.0}, 'Some Title'


class World:
    def __init__(self, slow_if, long_duration=500.79, snap_cost=0.0):
        self.t, self.last, self.slow_if, self.snap_cost = 1000.0, 1000.0, slow_if, snap_cost
        self.item, self.state, self.pos, self.pending = None, 'Stopped', 0.0, None
        self.seq = self.applied = self.sent = self.skipped = 0
        self.last_t, self.durations = None, dict(DURATIONS, **{'i.long1': long_duration})
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
                if self.pos >= self.durations[self.item]:
                    self.state = 'Stopped'
            self.last = end
            if self.pending and end >= self.pending['land']:
                p, self.pending = self.pending, None
                self.applied = p['seq']
                if p['kind'] == 'play':
                    self.state, self.pos = 'Playing', 0.0
                else:
                    self.pos, self.skipped = p['target'], self.skipped + 1000

    def snap(self):
        self.sync()
        self.t += self.snap_cost
        shown = self.pending['target'] if self.pending and self.pending['kind'] == 'seek' else self.pos
        return {'itemId': self.item, 'state': self.state, 'position': shown, 'duration': self.durations.get(self.item), 'applied': self.applied, 'seq': self.seq,
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
        target = float(args[1]) if args[0] == 'seek' else None
        if args[0] == 'play':
            self.item, self.state = args[1], 'Loading'
        self.pending = {'kind': args[0], 'seq': self.seq, 'target': target, 'land': self.t + (0.3 if args[0] == 'play' else self.slow_if(self.pos, target))}
        return 0, {'seq': self.seq, 'engine': 1}, 0


def run(slow_if, **world):
    w, noted = World(slow_if, **world), []
    d = types.SimpleNamespace(gate=gate, PACER=w.pacer, note_names=lambda found, kind: noted.append(found))
    ev = {'arms': {}}
    try:
        probe_seek.sequence(d, w, IDS, ev, w.clock, w.sleep)
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


if __name__ == '__main__':
    unittest.main(argv=sys.argv[:1] + [a for a in sys.argv[1:] if a.startswith('-')])
