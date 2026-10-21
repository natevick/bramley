# S39 Home fast: pre-registration (2026-10-21)

Committed before any measurement launch. Thresholds do not move after data comes in.

**Question.** S38 showed Home takes about 4.3 s to show 50 covers even with every cover in the art cache, because three staged ranking passes wait on live Apple requests. Do (1) Home's library pages issued concurrently with the history requests and (2) a disk-cached last Home grid make it fast?

**Arms.** Isolated harness: `scripts/app/covers-session.py` `cs.session` + `cs.isolated`, DISPLAY unset, null sink, no playback, `--route home --size 1100x720`, sink Guard on. One config and cache for the whole run.
- A = origin/main fbf052d, art cache warm.
- B = the new build, art cache warm and a cached Home grid present.
- C = the new build, art cache warm, no grid cache (parallel ranking alone). The harness deletes the grid file before every A and C launch and restores the grid left by the latest fill or C launch before every B launch.
- One fill launch (new build, cold art, not scored) warms the art cache and yields the grid file. Then A, B, C alternate ABCABC... until n >= 5 valid each. Spawns at least 20 s apart.

**Metrics** (seconds from just before spawn). first: first `texturesUploaded` >= 1. all: `texturesUploaded` >= 50 (cap 60 s). requests: `coversRequested` read 6 s after all, and equal to the value 2 s earlier. USS: peak Private_Clean + Private_Dirty over the process and children, sampled every 0.2 s from spawn to all + 6 s.

**Bars, fixed now.** Medians.
1. B all <= 1.5 s.
2. C all <= 75% of A all.
3. B first and C first <= 120% of A first.
4. B and C median requests per Home load <= A.
5. B and C peak USS <= A + 10 MiB.
Ship only if all five hold. Otherwise RESULTS says what dominates and stops.

**Did-it-run.** A launch counts only if Home was reached, a frame painted, 50 textures, guard clean, `artMisses` = 0 at all and at the end, `artHits` = 50 on A and C and >= 50 on B (a swap may decode more covers from the cache), requests stable, and `homeHits` = 1 on B and 0 on A and C. An invalid launch is replaced while budget remains; an arm below n = 5 is inconclusive.

**Budget.** At most 20 launches: 1 fill, 15 measured, 4 replacements.

**Named false positives.** Host load and OS page cache from other sessions (arms alternate; load recorded; a gap inside the within-arm spread is noise). B's first and all are the cached grid, not live ranking: B measures perceived speed, and the live pass is only judged through requests, USS and the did-it-run checks. Apple latency differs by moment (requests are counted, times are not corrected). Mock runs never count.
