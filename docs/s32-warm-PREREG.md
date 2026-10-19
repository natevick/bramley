# S32 cache warmer: pre-registration (2026-10-19)

Committed before any real measurement. Thresholds do not move after data comes in.

**Question.** Does `bramley warm`, run while the app is closed, make the first visit to a never-opened library album as fast as a repeat visit, without taking the app's slot or exceeding the request budget?

**Build.** natevick/s32-warm, `cargo build --release --features dev`. The cold arm uses the same binary and never runs `warm`. Every launch and warm run uses `cs.isolated` (own XDG_CACHE_HOME, config, keyring service), no playback, the real instance slot (refused if a real Bramley is running).

**Arms.** Route `album:<id>`, one album per launch. Order S, up to 5 warm runs, C1 W1 C2 W2 C3 W3 T, every spawn and run at least 70 s apart.
- S (seed): one app launch, route `songs`, in the W config before the first warm run, so the app records the display scale that warm needs (a fresh config has none). Not judged; its `frame` joins the first-frame baseline.
- C (cold): a fresh isolated config and empty cache per launch.
- W (warm): one isolated config. Up to 5 `bramley warm` runs, at least 60 s apart, stopping early on `stopped=complete`. `developer-token.json` is deleted before each W launch (the cache dir holds it only in W).
- T: the app launched about 2 s into a 6th warm run in the W config, route `songs`.

**Albums.** Population P = library albums whose track list and album-route cover entries are in the W cache after the warm runs (checked by file existence in scratch). C and W each draw 3 distinct albums from P with `random.Random(32)`. Ids stay in scratch only. P over the library size is reported as a count ratio.

**Metric** (seconds from spawn, status file read every 0.1 s, floor 0.1 s): `covers` = time of the last change of `library.artTextures` before a 6 s quiet window, cap 90 s, as in S29. `frame` = first `framesPainted`.

**Bar, fixed now.**
1. Median W `covers` <= 0.5 s. C median is reported (expected about 2 s) with within-arm spread.
2. Each warm run: `requests` <= 60, and the API paced at 1 s (checked by the unit tests and the printed counts).
3. T: `frame` <= 1.2 x the median `frame` of the S, C and W launches, the app holds the slot (no "already running"), and the overlapping warm run prints `stopped=app-running`.

**Did-it-run.** Warm runs: `cached` > 0 until one prints `complete`. A W launch counts only if its track list and cover entries existed before it; a C launch only if they did not; both reached the route, painted, and had `artTextures` > 0. An invalid launch is not repeated; an arm left with n < 2 makes the verdict inconclusive. Caps: 6 warm runs, 10 launches (8 planned).

**Budget conflict rule.** If no run prints `complete` by the 5th, the bar is judged on P as drawn, the coverage ratio is reported, and the conflict goes to the lead; it is not read as a pass for the whole library.

**Deviations from the brief, stated now.**
- An album's route cover is fetched right after its track list, not after every track list. Otherwise a library over about 250 albums starves covers for hundreds of runs. Grid covers (Albums cells, the library-albums tail of Home, playlist and song rows) come after all track lists.
- Art counts against the 60 but is not paced: it is one request at a time to the art CDN hosts (`*.mzstatic.com`), different hosts from `api.music.apple.com`. API calls stay at 1 per second.
- Only page 1 of Songs and Artists is warmed. Later Albums and Playlists pages are followed only when missing or older than 6 h.
- Home's ranking comes from listening history, which warm never requests, so warm covers only Home's library-albums fallback.

**Named false positives.** An album opened earlier in the session (already cached): C uses a fresh cache per launch and W never opens an album twice. Apple network bursts (S22): a gap inside the within-arm spread is noise. Page cache and DNS warmth: C and W interleave. The 0.1 s status floor: a W `covers` of 0.2 s means "in the second reading".

**Harness.** `scripts/app/s32-warm.py`; its ledger records every spawn and warm start.
