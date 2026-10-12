# S13 full acceptance harness: results (2026-10-12)

Pre-registration: `docs/s13-acceptance-PREREG.md`, with Amendments 1–3. Every verdict below was computed by `scripts/acceptance/gate.py` from the evidence in `target/acceptance/<run>/`. The counted runs are on the branch rebased onto main 8c6e77a (S8, S9b and S10 merged), from clean trees at 0c2a806 (Amendment 3).

## Verdict: v1 is not done
Two consecutive valid runs, `final-2` and `final-3`, both FAIL on the same three checks. Each has an app cause (below), not a harness one. Every other check PASSes in both runs, and every control holds.

| Check | final-2 | final-3 | Bar |
|---|---|---|---|
| F1 first frame (median of 5) | PASS 103 ms, sidebar 34 playlists | PASS 109 ms | ≤ 500 ms |
| F2 signed out | PASS: banner SignInNeeded, play refused (exit 5), no stream | PASS | |
| F3 search, CLI / view (median of 5) | PASS 243 / 487 ms | PASS 210 / 496 ms | ≤ 2 s |
| F4 audio start (median of 5) | PASS 858 ms; past 35 s, sent > 35 s, 355 s song | PASS 1073 ms | ≤ 3 s |
| F5 pause / seek / resume / next / prev | **FAIL** 19 / **538** / 59 / 288 / **545** ms | **FAIL** 20 / **535** / 38 / 289 / **548** ms | ≤ 500 ms |
| F6 album playthrough | PASS 3/3 gapless (skipped 0.0 s) | PASS 3/3 gapless | amendment (a) or (b) |
| F7 library | PASS 34 playlists; largest 250 tracks scrolled to the end with covers (dev-build leg) | PASS | |
| F8 queue ×3 | PASS exact order | PASS | |
| F9 MCP | PASS 10 tools; audio 798 ms | PASS 777 ms | ≤ 3 s |
| F10 failure paths | **FAIL**: bad id shows an error and the app lives; offline search errors; after the network returns, Home never reloads its covers in 90 s, while search recovers | **FAIL** (same) | |
| R1 PSS (zone 3) | PASS 173.7 (USS 145.9, RSS 207.4) | PASS 172.9 (145.1, 206.6) | ≤ 200 |
| R2 PSS (zone 3) | PASS 76.5 | PASS 75.7 | ≤ 100 |
| R3 soak growth, min 5→30 | PASS +1.0 PSS / +1.0 USS, 13 tracks, 10 changes | PASS +1.5 / +1.6, 13 tracks, 10 changes | ≤ 10 |
| R4 CPU playing (median) | PASS 3.5 % | PASS 4.0 % | ≤ 5 % |
| R5 CPU paused (median) | **FAIL 1.8 %** | **FAIL 2.0 %** | ≤ 1 % |
| R6 binary | 22,841,600 bytes | 22,841,600 | recorded |
| Ballast (USS rise vs MiB + kept covers) | holds: 70 + 1.76 → 72.3 | holds: 70 + 1.76 → 72.7 | ± 3.0 |
| Reference drift before / after | +1.1 / +1.4 | +1.4 / +1.1 | 5 |
| Co-tenant polls / sightings | 543 / 0 | 543 / 0 | |
| Leak grep (repo / run hits, plant) | 0 / 0, caught | 0 / 0, caught | |

## Controls (`final-controls`, `final-llvmpipe`)
Both runs are valid: no co-tenant, the leak grep is clean and caught its plant, and the reference stayed within 1.3 MiB.

| Control | Named checks | Outcome |
|---|---|---|
| No user token | F2 PASS; F4, F5, F6, F9, R1 FAIL (never started, not on missing evidence) | **holds** |
| Corrupted developer token (dev build, fixture URL) | F3 FAIL (CLI exit 1, view never shows results), F4 FAIL (MusicKit configure error) | **holds** |
| No stream (`PULSE_SINK` names a missing sink) | F4 FAIL: "playback never started"; the guard saw no stream and never fired | **holds** |
| Cut tail (`next` 3 s before the end of track 3) | F6 FAIL: cut transition skipped 1.5 s, not drained, landing within 1 s | **holds** |
| Leak (3 MiB per track) | R3 FAIL in zone 1: USS growth +36.5 MiB over 10 changes after minute 5; binary fails the shipped-feature check | **holds** |
| Missing artifact | every one of the 16 checks FAILs on an empty directory | **holds** |
| llvmpipe forced | F1, F4, R1, R2 all VOID on the renderer; llvmpipe seen in the samples (R1 would have read 204.6) | **holds** |

## FAILs, with causes (app; not fixed here, for routing)
1. **F5 seek and prev take ~540 ms to be acknowledged.**
   - `crates/engine/js/player.js` `enqueue()` raises `applied` to `n` only after `await f()`, that is after MusicKit's `seekToTime` / `skipToPreviousItem` promise settles. Those settle in ~0.5 s.
   - The effect shows at the same moment as `applied`: `appliedMs` equals `ms` in every rep.
   - pause (19 ms) and resume (36–59 ms) are fast; next is 288 ms in most rounds.
   - **Options:** acknowledge on start; or treat it as a MusicKit floor and take the 0.5 s bar to Nate.
2. **F10: Home never recovers after the network returns.**
   - `covers::Grid::follow` fetches the charts only when the pixel size changes. A failed fetch is never retried, so the "Some covers did not load" state lasts until restart.
   - Search recovers, because it runs on demand.
   - Offline, search errors cleanly, the Search view shows `error`, and the app stays alive. A bad id (`play 1`) shows `Error("setQueue: NOT_FOUND …")`, and the app stays alive.
3. **R5: 1.8–2.0 % CPU while paused** (bar 1 %).
   - It's the same with and without `--status`.
   - Guard A/B on an idle, never-played app: an idle, never-played shipped app reads **0.0 %** with the guard on, off and on again (30 × 2 s each). So the guard costs nothing measurable, and neither does the UI's idle repaint. The paused cost comes from the started playback engine.
   - The cost is the engine while paused (its JS runtime and player thread keep ticking). This is a measured app cost, not a harness one.

## Deviations still needing approval (PREREG)
- **F7:** open and scroll on the dev build. There is no shipped command for either; the alternative is an app story.
- **F4/F9 "decrypted seconds":** checked through `sentMs` on a 355 s song. **R3 "underruns":** a position-stall proxy. The counters exist only behind the engine's `diagnostics` feature.
- **R4/R5** are measured without `--status`.
- **Corrupted developer token on the dev build:** the shipped path renews from the CI URL and heals itself.
- **Medians** for F1 and for F5's network commands.

## Runs that don't count (reported, not dropped)
| Run | Base | Gate | Why |
|---|---|---|---|
| s13-1 | pre-S9b | FAIL | F5, F10, R5, plus R3 (two "Loading" samples mid-track while the position kept advancing) and R4 6.5 %. A non-gated S9b live session ended inside its soak; the then-harness could not see it. Leak grep: 4 false hits on an ordinary-word playlist name. |
| s13-2 | pre-S9b | FAIL | S9b's gate started inside its pre-flight window: reference −13.8, R1–R3 VOID, the ballast mis-sized. |
| s13-3 | pre-S9b | FAIL | clean; F5, F10, R5 only |
| s13-4 | pre-S9b | VOID | the coordinator's `norm-s9b` gate ran 22:41:51–22:47:10, inside this run. **Its R1/R2 readings overlap mine and are suspect.** |
| s13-controls, s13-llvmpipe | pre-S9b | controls all hold | gate FAIL only from the leak-grep plant (no library names in those modes; fixed in Amendment 1) |
| final-1 | final | VOID | one-poll co-tenant race on a short-lived CLI pid (Amendment 3); every number matched final-2 |

## Harness
- **Files:** `scripts/acceptance.sh`, `scripts/acceptance/{drive,gate,test_gate,mutants}.py`, and a trimmed `scripts/app/covers-session.py`. s7, s8 and s10, `mcp-session`, `run.sh` and `s2a_verdict` are removed.
- **Lines:** 2,076 before (harness 1,885 + tests 191); 1,863 (harness 1,363: `acceptance.sh` 31, `drive.py` 700, `gate.py` 457, `covers-session.py` 175; tests and mutant runner 500) after. Net −213. That is still over the plan's 600-line harness cap.
- **Tests:** `test_gate.py` has 11 tests and about 300 subcases, one per rule and per check's missing evidence. Mutation score: 47 of 47 killed, with the no-op control surviving.
- **App changes:**
  - `bramley search` from the plain CLI fills the Search view (an MCP search doesn't);
  - the search view's `seq` is in the status;
  - `gate-sabotage` gains `BRAMLEY_GATE_LEAK_MIB`.
