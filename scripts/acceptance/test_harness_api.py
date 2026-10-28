import importlib.util, io, json, os, sys, unittest, urllib.error

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location('s9_session', os.path.join(HERE, '..', 'app', 's9-session.py'))
s9 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(s9)
_drive = importlib.util.spec_from_file_location('drive_under_test', os.path.join(HERE, 'drive.py'))
drive = importlib.util.module_from_spec(_drive)
_drive.loader.exec_module(drive)
TOKENS = ['developer', 'user']


class Reply(io.BytesIO):
    def __init__(self, body):
        super().__init__(json.dumps(body).encode())


class Apple:
    def __init__(self, *answers):
        self.answers, self.paths, self.sleeps = list(answers), [], []

    def open(self, req, timeout):
        self.paths.append(req.full_url)
        answer = self.answers.pop(0)
        if isinstance(answer, int):
            raise urllib.error.HTTPError(req.full_url, answer, 'denied', {}, None)
        return Reply(answer)

    def call(self, path='/v1/me/library/albums?limit=100'):
        return s9.api(path, TOKENS, opener=self.open, sleep=self.sleeps.append)


class HarnessApi(unittest.TestCase):
    def test_two_denials_then_success_returns_the_body_after_backing_off(self):
        apple = Apple(403, 403, {'data': [1]})
        self.assertEqual(apple.call(), {'data': [1]})
        self.assertEqual((len(apple.paths), apple.sleeps), (3, [2, 5]))

    def test_a_401_is_retried_the_same_way(self):
        apple = Apple(401, {'data': []})
        self.assertEqual(apple.call(), {'data': []})
        self.assertEqual(apple.sleeps, [2])

    def test_three_denials_raise_a_harness_api_error_naming_the_code_and_the_endpoint_shape(self):
        apple = Apple(403, 403, 403)
        with self.assertRaises(s9.HarnessApiError) as raised:
            apple.call()
        e = raised.exception
        self.assertEqual((e.code, e.path_kind, str(e), len(apple.paths)), (403, 'library/albums', 'HTTP 403 on library/albums', 3))

    def test_the_endpoint_shape_drops_ids_names_and_the_query(self):
        for path, want in (('/v1/me/library/playlists/p.AbC123/tracks?limit=1', 'library/playlists/tracks'),
                           ('/v1/catalog/us/albums/1440857781/tracks', 'catalog/albums/tracks'),
                           ('/v1/me/library/artists/r.Zz9/albums?limit=100&offset=100', 'library/artists/albums')):
            with self.subTest(path):
                self.assertEqual(s9.path_kind(path), want)
                with self.assertRaises(s9.HarnessApiError) as raised:
                    Apple(403, 403, 403).call(path)
                self.assertEqual(raised.exception.path_kind, want)

    def test_other_client_errors_are_not_retried_and_are_not_harness_errors(self):
        apple = Apple(400)
        with self.assertRaises(RuntimeError) as raised:
            apple.call()
        self.assertNotIsInstance(raised.exception, s9.HarnessApiError)
        self.assertEqual((len(apple.paths), apple.sleeps), (1, []))

    def test_rate_limits_and_server_errors_still_retry_after_two_seconds(self):
        apple = Apple(429, 503, {'data': []})
        self.assertEqual(apple.call(), {'data': []})
        self.assertEqual(apple.sleeps, [2, 2])

    def test_a_404_is_an_empty_library(self):
        self.assertEqual(Apple(404).call(), {'data': [], 'meta': {'total': 0}})

    def test_a_phase_that_died_on_a_harness_api_error_is_recorded_typed_and_others_stay_strings(self):
        self.assertEqual(drive.phase_error(drive.s9.HarnessApiError(403, 'library/albums')), {'kind': 'harness-api', 'code': 403, 'path': 'library/albums'})
        self.assertEqual(drive.phase_error(RuntimeError('HTTP 400 on a library call')), 'RuntimeError: HTTP 400 on a library call')
        self.assertEqual(drive.phase_error(ValueError('boom')), 'ValueError: boom')

    def test_every_sample_records_the_reclaim_counters_the_split_the_anon_size_and_the_host_memory(self):
        class Fake:
            pid = os.getpid()
            cli = staticmethod(lambda *args: (0, {'state': 'Playing'}, 0))
        rows = drive.sample(Fake, 2, every=0.0)
        self.assertEqual(len(rows), 2)
        for row in rows:
            self.assertTrue(row['vmstat'] and all(k.startswith(('pgscan', 'pgsteal')) and isinstance(v, int) for k, v in row['vmstat'].items()))
            self.assertEqual([p['pid'] for p in row['split']][0], Fake.pid)
            self.assertEqual(len(row['split']), row['procs'])
            self.assertTrue(all(p['shared'] >= 0 and p['private'] > 0 for p in row['split']))
            self.assertTrue({'pss', 'uss', 'rss'} <= set(row))
            self.assertTrue(0 < row['anon'] <= row['rss'])
            self.assertTrue(0 <= row['swap'] and row['anon'] + row['swap'] > 0)
            self.assertEqual(set(row['host']), {'available', 'free', 'swapFree', 'shmem', 'activeFile', 'inactiveFile'})
            self.assertTrue(all(isinstance(v, int) and v >= 0 for v in row['host'].values()))



class GuardStalls(unittest.TestCase):
    def setUp(self):
        import subprocess, time
        self.time, self.cs = time, drive.cs
        self.child = subprocess.Popen(['sleep', '30'])
        self.real, self.stall = self.cs.pw_dump, self.cs.PW_STALL_S
        self.cs.PW_STALL_S = 1

    def tearDown(self):
        self.cs.pw_dump, self.cs.PW_STALL_S = self.real, self.stall
        self.child.kill()
        self.child.wait()

    def guard(self, answers, seconds):
        def dump():
            answer = next(answers)
            if isinstance(answer, Exception):
                raise answer
            return answer
        self.cs.pw_dump = dump
        g = self.cs.Guard(self.child.pid, 'bramley-gate-null')
        started = self.time.time()
        g.start()
        g.join(seconds)
        g.stop = True
        g.join(2)
        return g, started

    def timeout(self):
        import subprocess
        return subprocess.TimeoutExpired(['pw-dump'], 1)

    def test_one_slow_dump_is_retried_and_does_not_kill_the_app(self):
        import itertools
        g, _ = self.guard(itertools.chain([self.timeout()], itertools.repeat([])), 1.5)
        self.assertIsNone(g.killed)
        self.assertEqual(len(g.stalls), 1)
        self.assertGreater(g.polls, 0)
        self.assertIsNone(self.child.poll())

    def test_a_dump_that_never_answers_kills_the_app_at_the_deadline_not_before(self):
        import itertools
        g, started = self.guard(itertools.repeat(self.timeout()), 3)
        self.assertIn('pw-dump failed for 1 s', g.killed['why'])
        self.assertGreaterEqual(g.killed['t'] - started, 1)
        self.assertLess(g.killed['t'] - started, 2)
        self.assertEqual(self.child.wait(2), -9)

    def test_the_deadline_restarts_at_every_answer(self):
        import itertools
        burst = [self.timeout()] * 3 + [[]]
        g, _ = self.guard(itertools.cycle(burst), 2.5)
        self.assertIsNone(g.killed)
        self.assertGreaterEqual(len(g.stalls), 6)
        self.assertIsNone(self.child.poll())

    def test_a_stream_on_the_wrong_sink_is_still_killed_on_the_first_answer(self):
        import itertools
        pid = self.child.pid
        dump = [{'id': 1, 'type': 'PipeWire:Interface:Node', 'info': {'props': {'application.process.id': pid, 'media.class': 'Stream/Output/Audio'}}},
                {'id': 2, 'type': 'PipeWire:Interface:Node', 'info': {'props': {'node.name': 'speakers', 'media.class': 'Audio/Sink'}}},
                {'id': 3, 'type': 'PipeWire:Interface:Link', 'info': {'output-node-id': 1, 'input-node-id': 2}}]
        g, started = self.guard(itertools.repeat(dump), 1)
        self.assertEqual(g.killed['why'], "stream linked to ['speakers']")
        self.assertLess(g.killed['t'] - started, 0.5)

    def test_the_production_deadline_is_five_seconds(self):
        self.assertEqual(self.stall, 5)


if __name__ == '__main__':
    unittest.main(argv=sys.argv[:1] + [a for a in sys.argv[1:] if a.startswith('-')])
