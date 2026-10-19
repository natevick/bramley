# S32 cache warmer: results (2026-10-19)

Judged against `docs/s32-warm-PREREG.md` (final text d4486b2, committed 10:52:39). First measurement: the seed launch, spawned 10:53:52, 73 s later. Build: tree `c827be5f`, clean, `--release --features dev`, sha256 `623305c8`. 6 warm runs and 8 launches, every spawn at least 70 s after the last, no playback, isolated config and cache; the real `~/.cache/bramley` was never created.

**Verdict: bar met on the albums warm had reached, not on the library.** No run printed `complete`, so per the conflict rule the bar is judged on P (134 of 376 library albums, 36%) and the coverage goes to the lead.

| arm | n | covers (each, s) | median | spread |
|-----|---|------------------|--------|--------|
| C cold | 3 | 2.01 2.11 1.61 | 2.01 | 0.50 |
| W after 5 warm runs | 3 | 0.2 0.3 0.2 | 0.20 | 0.10 |

1. W median 0.20 s against the 0.50 s bar. The 0.1 s status floor means 0.2 is "in the second reading".
2. Requests per warm run: 59, 59, 59, 60, 60, and 2 for the run an app interrupted. All at most 60. The 1 per second pacing is held by `requests_are_paced`, not by these counts.
3. T: the app spawned 2 s into a warm run reached its first frame at 0.1 s against a limit of 0.24 s (1.2 times the S, C and W median of 0.2 s). It held the slot. The warm run printed `requests=2 stopped=app-running`.

**Did it run.**
- Every warm run printed `cached` at least 59 until it stopped on the budget.
- All three W launches had their track list and covers in the cache beforehand and showed `artMisses` 0, `artHits` 51 to 64.
- All three C launches had neither, and showed `apiMisses` 2.
- Albums were drawn from P with `random.Random(32)`; ids stay in scratch.

**Warm run lines.**

| run | requests | cached | stopped |
|-----|----------|--------|---------|
| W1 to W3 | 59, 59, 59 | 59, 59, 59 | budget |
| W4, W5 | 60, 60 | 60, 60 | budget |
| T | 2 | 2 | app-running |

`skipped` is large (285 to 379) because it counts every item not requested, including the 23 other tracks that share an album's cover URL.

**Limits.**
- Coverage: about 25 albums per run (a track list and a cover each). 376 albums need about 15 runs, so about 75 minutes of timer from a cold start; 1000 albums about 3 hours. After that a run mostly refreshes the list pages and the oldest entries.
- n=3 per arm and a 0.1 s clock. The cold median of 2.0 s matches S29's 1.9 to 2.1 s.
- The warm arm reads cache hits decoded locally; this shows the cache works, not that Apple stays quiet over days.
- Stop latency on an app start is bounded by the request in flight (15 s API, 30 s art timeouts); here it stopped before the third request.
- Not exercised against Apple: a 429, 5xx, 401 or 403 stop, the keyring-locked path, and the timer itself. Those are covered by unit tests, the mock servers and `systemd-analyze verify` only.
- The art tier needs the display scale the app records. A user who has never run this build gets lists and track lists but no covers until the app has run once.
- Windows and macOS compilation was not checked: only the Linux target is installed.
