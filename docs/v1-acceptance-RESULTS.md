# v1 Linux acceptance: PASS (2026-10-13)

The test half of the v1 bar in `acceptance.md` is met: `scripts/acceptance.sh` PASSed twice in a row on `main` (07be8d1), and the controls run held. The other half is met too: Nate saw the screenshots, sent by Taildrop, and confirmed on 2026-10-13 ("Yes they look right"). **v1 Linux is done.**

| Run | Mode | Start–end (PT) | Gate |
|---|---|---|---|
| v1-1 | counted | 2026-10-13 morning, at 3106814 | FAIL: F4 (Apple refused a license; see `s14d-cold-play-RESULTS.md`), and the leak-grep self-test could not run on `main` (fixed in Amendment 8) |
| v1-2 | counted | 11:48–13:00 | **PASS**, 16 of 16 |
| v1-controls | controls | 13:00–13:50 | **PASS**, every control held |
| v1-3 | counted | 13:50–15:00 | **PASS**, 16 of 16 |

All three runs at 07be8d1: clean tree, real state unchanged, no phase errors, the leak grep clean (275 library names, both plants caught), and pacing at a peak of 20 license-requesting actions per 600 s (Amendment 9).

## Numbers (v1-2 / v1-3)

| Check | Bar | v1-2 | v1-3 |
|---|---|---|---|
| F1 first frame | ≤ 500 ms every launch | 124 ms median | 113 ms median |
| F3 search | ≤ 2 s | 228 ms CLI / 475 ms UI | 175 / 349 ms |
| F4 cold play to audio | ≤ 3 s | 2,213 ms | 1,970 ms |
| F5 pause / seek / resume / next / prev | ≤ 0.5 s each | 30 / 351 / 277 / 177 / 159 ms | 26 / 334 / 278 / 162 / 173 ms |
| F9 MCP play to audio | ≤ 3 s | 618 ms | 575 ms |
| R1 playing | ≤ 200 MiB | 174.0 PSS (zone 3) | 174.3 PSS (zone 3) |
| R2 browsing | ≤ 100 MiB | 75.6 PSS | 76.1 PSS |
| R3 30-min soak | growth ≤ 10 MiB, no underruns | +0.7 MiB, 13 tracks, 0 underflows, 0 readiness dips | +3.1 MiB, 13 tracks, 0, 0 |
| R4 CPU playing | ≤ 5 % | 4.1 % | 4.1 % |
| R5 CPU paused | ≤ 1 % | 0.6 % | 0.6 % |
| R6 binary size | reported | 22,919,168 bytes | same |

F2, F6, F7, F8 and F10 PASSed in both runs.

Controls (v1-controls): no user token, corrupted developer token, no stream, cut tail, leak, stall (SIGSTOP), guard kill and missing artifact each failed for its stated reason.

Evidence stays local, in `target/acceptance/{v1-1,v1-2,v1-controls,v1-3}/` of the run worktree. Screenshots with real art stay in `target/screens/PRIVATE-real-art/`, are never committed, and went to Nate by Taildrop.

## Known limits
- Apple throttles license requests per account (about 30 in 8 minutes). The harness paces itself; the app shows "playback did not start within 10 s" if a start stalls.
- Linux x86_64 only. macOS and Windows compile but are not measured.
