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
_First-round list, written for final-2/final-3. Amendments 4–6 replaced several of these checks, and the v1 run reconciles what still applies._
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

## Fix round (2026-10-12): rules from PREREG Amendments 4–5, at 75f937c
**Counted runs:** `fix-2` and `fix-3` (consecutive), then `fix-controls`. All three held the GPU lock, saw no co-tenant in 517–674 polls, had no phase errors, and had a clean leak grep with its file plant caught. `fix-1` (at 5fae888) is reported, not counted (see Amendment 5).

**Verdict:** v1 is still not done. Every FAIL below has an app cause, and every control fired on its stated reason.

| Check | fix-2 | fix-3 | Bar |
|---|---|---|---|
| F1 slowest of 5 launches | PASS 114 ms | PASS 142 ms | each ≤ 500 |
| F2 | PASS | PASS | |
| F3 CLI / view (median) | PASS 245 / 498 ms | **FAIL**: rep 2 hit the CLI's 3 s apply deadline ("the app did not answer in time", 4,004 ms); the other reps 148–289 ms | every rep returns songs; median ≤ 2 s |
| F4 cold first play (median of 5 launches) | PASS 1,971 ms; decrypted 38.7 s per play, skipped 0, pace ≤ 0.05 s | PASS 2,003 ms; decrypted 38.7 s | ≤ 3 s, plus the judged-play deltas |
| F5 pause / seek / resume / next / prev (effect) | **FAIL** 19 / **519** / 280 / 279 / **532** ms | **FAIL** 25 / **780** / 287 / **561** / **810** ms | pause, resume each ≤ 500; others median ≤ 500 |
| F5 next / prev new item advancing (median) | 531 / 780 ms | 808 / 1,065 ms | ≤ 3,000 |
| F6 | PASS 3/3 gapless (sent at flip 19.5–20.6 s, skipped 0, stall 0.24 s) | PASS | Amendment 4 |
| F7 | PASS (the dev-build leg) | PASS | |
| F8 | PASS | PASS | |
| F9 (median) | PASS 1,047 ms; decrypted 38.8 s | PASS 1,064 ms | |
| F10 | **FAIL**: no Home recovery | **FAIL** | |
| R1 PSS | PASS 174.5 | PASS 172.5 | ≤ 200 |
| R2 PSS | PASS 77.4 | PASS 76.7 | ≤ 100 |
| R3 growth | +1.0 PSS / +1.0 USS, 13 tracks; **FAIL**: 51 underruns | −0.6 / −0.7 … +0.8, 13 tracks; **FAIL**: 47 underruns | growth ≤ 10; underruns = 0; pace ≤ 1 s |
| R4 CPU playing, 10 s bins (median / mean) | PASS 3.7 / 3.74 % | PASS 3.9 / 4.10 % | ≤ 5 % |
| R5 CPU paused, 10 s bins (median / mean) | **FAIL** 1.8 / 1.80 % | **FAIL** 1.8 / 1.84 % | ≤ 1 % |
| R6 | 22,843,520 bytes | same | recorded |
| Ballast (rise against MiB + 1.76) | holds: 68 → 68.6 | holds: 70 → 71.4 | ± 3.0 |

**Controls (`fix-controls`): every one held.**

| Control | What fired |
|---|---|
| No user token | F2 PASS. F4 never started, under `SignInNeeded`. F9 never started (F9 records no banner). F5 had 0 rounds; F6 had 0 of 3 transitions. R1 never started |
| Bad-signature token (a real JWT, signature reversed) | F3 failed with "developer token was rejected" (Apple's 401). F4 never started; the banner read `SignInNeeded`. **That F4 reason can't be told apart from the no-user-token control**; S14c fixes the banner priority |
| No stream | F4 never started: "the audio output could not be opened" |
| Cut tail | F6 failed 2 of 3; the cut transition was caught by its skip count |
| Leak | R3 FAIL in zone 1, USS +13.7 MiB. Only 5 changes after minute 5 (10 in earlier runs), above the 4 required. **The soak froze from 16:35:42 to 16:48:47**, on one item at position 383.8 s, in Playing with no banner (see finding 7) |
| Stall (SIGSTOP for 3 s) | the pacing rule caught a 2.92 s lag. `underruns` stayed at 0 |
| Guard kill (a second, existing null sink) | the guard killed the app with "stream linked to" that sink; the app was gone; the default sink was unchanged |
| Missing artifact | all 16 checks FAILed on missing evidence |

**New or confirmed app findings (for routing; none fixed here)**
1. **F5:** seek and prev settle only after MusicKit's promise. `applied` lands with the effect at 520–810 ms.
2. **R3: the engine's underrun counter rises about once per 10 s mid-track on certain tracks**, with no pacing lag (≤ 0.2 s):
   - two tracks had 16 each, and two had 7 and 5, out of 47–51 per soak;
   - the media element repeatedly drops below `HAVE_FUTURE_DATA` while the output keeps playing.
3. **F3:** one CLI search in 5 took more than the CLI's 3 s apply window, and was reported as "not applied".
4. **The bad developer token shows `SignInNeeded`:** the developer-rejected banner is outranked in `app.rs` `banner()`.
5. **R5:** 1.8 % CPU while paused. An idle, never-played app reads 0.0 %, so the cost is the started engine.
6. **F10:** Home never reloads after the network returns.
7. **Playback freezes at an item transition.**
   - In fix-1, F8 and R3 stalled with `seq` frozen while 18 commands exited 0.
   - In fix-controls, the leak soak froze from 16:35:42 to 16:48:47 on one item, in Playing with no banner.
   - A smoke repro, fix-2 and fix-3 played normally.
   - Very likely race 3, which S14b fixed on its branch. Queue replies are now recorded.

**Ballast note:** across runs the rise residual (measured minus requested plus kept) spans −3.3 to +0.9 MiB. fix-1's −3.3 missed the 3.0 tolerance. This is launch-to-launch USS variance; the tolerance was not widened. **Proposal:** allocate the ballast inside the same launch after a first R1 window, so the rise is measured within one process.

**After the fix round (Amendment 6)**
- **The leak grep was blind in fix-controls:** it fetched 0 library names, because Apple was refusing the real user token (HTTP 403 on `/v1/me/storefront` and the library API) by 16:49. Library names are now required.
  - **Correction:** this and Amendment 6 said the leak soak "was still playing" at ~16:47. It wasn't: it had frozen at 16:35:42. So the start of the token refusal is not bracketed by playback evidence.
- **Re-scored:** fix-controls' gate is FAIL on the blind grep, though every control still holds. fix-2 and fix-3 (275 names each) are unchanged.
- **Committed docs:** grepped again at the final commit. Tokens: 0 hits. Names couldn't be fetched, for the same reason.
- **Before any further signed-in run,** Nate must sign in again. The real token file is unchanged since 2026-10-09, so this is Apple's refusal, not a harness write.

**Deviation pending the coordinator's approval: F9 under Amendment 5**

Amendment 5 moved the F4/F9 skipped baseline from the command to the first audible snapshot, after fix-1 (which Amendment 4 had named counted) had run. That is a loosening.
- **Under Amendment 4, F9 FAILs:** fix-1 1,513 ms, fix-2 1,510 ms, fix-3 1,463 ms skipped.
- **Under Amendment 5, F9 PASSes.**
- **No gate verdict flips:** every run FAILs anyway, on F5, F10, R3 and R5.

What was skipped: the previous song's queued audio, flushed at the switch. The new song's head was not cut.

| Judged play | Position at first audible snapshot | skippedMs at command / first audible / end | Decrypted before first audible |
|---|---|---|---|
| fix-1 F9 | 0.67 s | 4,435 / 5,948 / 5,948 | 2.28 s |
| fix-2 F9 | 0.50 s | 4,520 / 6,030 / 6,030 | 2.14 s |
| fix-3 F9 | 0.46 s | 4,474 / 5,937 / 5,937 | 2.09 s |
| cold F4, 15 launches (fix-1–3), for comparison | 0.07–0.50 s | 0 / 0 / 0 | 1.51–2.02 s |

- **All of the jump comes before audio starts:** the ~1.5 s rise lands between the command and first audible, and nothing is skipped after.
- **The new song starts near its head:** at 0.46–0.67 s, which is within or just above the cold-play range. A cut head would show the song starting at about 1.5 s.
- **Limit:** F9 judges only its last rep, so first-audible positions exist for that rep only, one per run.

**Decision, 2026-10-12, coordinator (Norm): APPROVED.** The skipped audio is the previous item's queue, flushed before the new item is audible. The new item starts within ~0.2 s of the cold-play range, so the head is not cut, and Amendment 5 measures the property F4/F9 actually name. The loosening is kept on the record. If a future run shows the new item first audible at ≥ 1.0 s, this approval is void and the cause is investigated as a cut head.

## S14c (2026-10-12): R3 on output underflows, banner priority, in-launch ballast; F3 and fix-1 findings
Rules: PREREG Amendment 7, committed before any code. No signed-in run was possible: Apple refuses the real user token (HTTP 403 on `/v1/me/storefront`), so nothing below was run live.

**(a) R3 gates output starvation.**
- The engine counts PulseAudio `UNDERFLOW` (command 63: channel, then `s64` offset from protocol v23) reported by the server for our stream. It shows in the status as `outputUnderflows`. The readiness-dip counter keeps its meaning, renamed `readinessDips`.
- R3 FAILs on any `outputUnderflows` rise from the first sample to the last, or when either sample lacks the counter. Pacing is unchanged; `readinessDips` is reported, never gated.
- The stall control holds on an underflow and/or a pacing lag.
- **Tests:**
  - a fake Pulse server sends `UNDERFLOW` and exactly 1 is counted, with no failure;
  - the media element adds every underflow its sink reports;
  - slot order runs through `output_counts`, the snapshot and the status keys.
- **Harness:** 7 new matrix rows and 1 stall test in `test_gate.py`, and 13 new mutants.

**(b) Banner priority.**
- `banner()` now ranks the developer-token-rejected banner above the playback banner.
- **A second cause, found by the test:** a `SignInNeeded` snapshot reset the cover grid, which wiped its `developer_rejected` flag. The reset now keeps the flag. Reverting either change alone brings `SignInNeeded` back.
- The bad-token control now expects "developer token was rejected" in every F4 banner.

**(c) Ballast in one launch.**
- The gate-sabotage build allocates nothing at start. It touches the ballast once, when the file named by `BRAMLEY_GATE_BALLAST_FILE` appears, and reports `ballastMiB`.
- The harness takes an R1 window, writes the trigger atomically, waits for `ballastMiB` to match, then takes the judged window. The rise is post − pre in one pid; the expected rise is size + kept covers added between the windows.
- The tolerance stays 3.0 MiB, and the sizing rule is unchanged.

**Mutation (all under the GPU lock):**
- **Rust:** 12 of 12 killed, and the no-op survived. Each test below fails without its fix.

| Mutant | Killed by |
|---|---|
| `UNDERFLOW` arm removed from the reader | `an_underflow_the_server_sends_is_counted_and_is_not_a_failure` |
| underflow not counted / never taken | same |
| `pump` never harvests; harvest overwrites | `every_server_underflow_the_sink_reports_is_counted_once` |
| `output_counts` slots swapped | `output_counts_keep_each_counter_in_its_own_slot` |
| snapshot slots swapped | `snapshots_use_native_time_and_map_errors_redacted` |
| status keys swapped | `engine_counts_reach_their_own_status_keys` |
| banner order reverted; reset forgets the rejection | `a_rejected_developer_token_outranks_sign_in_needed_from_playback` |
| ballast re-arms; ballast pages untouched | `the_ballast_sabotage_touches_its_size_once_when_its_trigger_appears` |

- **Gate (`mutants.py`):** 117 of 117 killed, and the no-op survived. The baseline was 106.

**(d) F3 in fix-3: one CLI search hit the 3 s window. Finding: a single slow HTTP response to Apple, not our queue or IPC; not fixable from this evidence.**
- **The CLI and IPC path delivered the request.** 4,004 ms is exactly `APPLY_TIME` (3 s) + `REPLY_MARGIN` (1 s): the app's instance handler accepted the line, queued it and waited out its full window (`cli.rs` `handler`). A transport failure would have returned at once.
- **The app ran the search, late.** The Search view showed that search's results (`seq` moved, results > 0) 5,287 ms after the command.
- **Not the worker queue.**
  - A CLI search queues two jobs on the single API worker: the CLI's, then the Search view's (`enter()` makes the view's job due at once).
  - In the other 49 reps across ten runs, view − CLI was 159–294 ms, one request apart.
  - Nothing else was queued: the driver waits for the sidebar's single playlist page (34 of 34, no more), songs, albums and artists had 0 pages, covers use their own art agent, and MCP (F9) ran after F3.
  - So one HTTP call took about 5 s.
- **Not a dead pooled socket.** fix-3's binary (75f937c) contains neither B1 pool fix (a364e62, 16ecf45 are not its ancestors). In that binary:
  - a silently dead pooled socket ends in the 15 s global timeout;
  - a reset one errors at once.
  - Neither ends in a success after about 5 s, and main's B1 fixes don't change a slow response that succeeds.
- **What remains:** Apple's response or the network path (connect, TLS). The shipped build logs no per-request timing, so these can't be told apart. It occurred in 1 of 50 CLI searches across ten runs; the other 49 took 144–469 ms. IPv6 to the API fails in 1 ms here (no route), so no v6 connect hang is in play.
- **Routed, not fixed (ours, still on main):** the CLI reports "the command was not applied", yet for a search the app went on and showed the results. The message is wrong for API requests.

**(e) fix-1's frozen `seq`: finding: not a frozen `seq`; consistent with, but not shown to be, race 3.**
- **`seq` 85 includes every R3 command.** Counting the shipped session's commands: R1 2, warm F4 10, F5 40, F6 8, F8 6, R3 19 = 85. So the 18 R3 commands (`play` + 17 `queue add`) were all registered, and `applied` reached 85.
- **`applied` does not prove effect.** `player.js` starts each command's turn after at most `commandWaitMs` (5 s), even if the previous MusicKit promise never settled, and raises `applied` then. A MusicKit that never settles still lets `applied` reach `seq`, with no banner.
- **Where it stopped:** F8 saw only its first item (album track 10), then ran out its 120 s. Every R3 sample, from minute 0 to 30, was `Stopped`, with no item and no queue length.
- **Race 3 (S14b) is a permanent `Loading` with the item present and the position frozen at a preload.** fix-1 recorded no F8 snapshots, so whether it passed through `Loading` at that transition is unknown. The end state (`Stopped`, no item) is not the registered race-3 signature.
- **For race 3:**
  - the failure began at an item transition right after a seek to duration − 5, which is where race 3 fires;
  - fix-1's binary (5fae888) predates S14b's HTTP cache (5438da1 is not its ancestor).
- **Against race 3, or unexplained:** the subsequent fresh `setQueue` + `play` never brought MusicKit back. Race 3 as S14b traced it stalls one preload; it does not explain a new queue that never starts.
- **Verdict:** unproven. Next time, the queue replies Amendment 5 now records and an F8 snapshot series would settle it.

**Live checks still owed** (need Nate's fresh sign-in):
1. The stall control fires on `outputUnderflows` (≥ 1) as well as on pacing, and its stated reason reads "output underflow and/or pacing lag".
2. R3 on main: `outputUnderflows` stays 0 across seeks on pipewire-pulse 1.6.2, and `readinessDips` is reported. Any underflow clustered at seeks or drains is investigated (Amendment 7).
3. The in-launch ballast residual falls within 3.0 MiB, and `ballastMiB` is confirmed.
4. The bad-token control's F4 banners read "developer token was rejected" (this depends on the 401 landing before the play's snapshot).
5. A fresh counted pair plus `--controls` at this commit or later.
