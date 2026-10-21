# S39 Home fast: results (2026-10-21)

Judged against `docs/s39-home-fast-PREREG.md` (commits 1064e03 and fb7235d, 11:59 and 12:02, first launch 12:14:25), unchanged since. Linux, 24 cores, headless weston, null sink, no playback, isolated config and cache, `scripts/app/s39-home-fast.py`. 16 launches of 20: 1 fill (cold art, not scored), 15 measured, all valid. A = origin/main fbf052d (sha256 afc2b9de...), B and C = a174be3 (sha256 f4b30802...). Load average at launch fell from 10 to 2 over the run; arms alternated A B C.

| median (spread) | A | B | C |
|---|---|---|---|
| first cover s | 1.320 (0.309) | 0.440 (0.301) | 1.373 (0.464) |
| all 50 s | 4.499 (0.682) | 0.440 (0.310) | 2.748 (0.245) |
| requests per Home load | 9 | 9 | 9 |
| peak USS MiB | 58.29 (3.05) | 55.50 (2.16) | 55.08 (1.67) |

| bar | result | held |
|---|---|---|
| B all <= 1.5 s | 0.440 | yes |
| C all <= 75% of A | 0.611 | yes |
| B, C first <= 120% of A | 0.333, 1.040 | yes |
| B, C requests <= A | 9, 9 vs 9 | yes |
| B, C USS <= A + 10 MiB | -2.79, -3.21 | yes |

Did-it-run: every measured launch had artHits 50 (A, C; B at least 50), artMisses 0 at all and 2 s later, a stable request count, and homeHits 1 on B, 0 on A and C. No B launch swapped the grid (artHits stayed 50): live ranking matched the cache every time.

**Verdict: ship.** All five bars hold. C's all-50 gap to A (1.75 s) is far outside either spread, so parallel ranking alone is real; B is the cached grid on screen once the storefront check answers.

**Limits.**
- B's time is the cached grid, not live ranking. The live pass shows only through requests, USS and the no-swap check.
- This account's history never filled 50 albums, so the one-extra-library-request case (history alone fills the grid) was not exercised; it is covered only by reasoning and by `home_asks_replay_...` counting 9 on the fixture.
- C's first cover is unchanged (1.373 vs 1.320 s): the first pass still waits on the storefront check, Replay and the first lookup.
- `bramley warm` does not refresh the grid. That needs the history requests, which compliance says warm never makes.
