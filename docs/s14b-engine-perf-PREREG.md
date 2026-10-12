# S14b engine latency and paused CPU: pre-registration (2026-10-12)

S13 found two engine-side FAILs: R5 (paused CPU 1.8–2.0 %, bar 1 %) and F5 (seek ~538 ms, prev ~545 ms, bar 0.5 s). The bars in `docs/acceptance.md` are binding and do not move here. Written before any after-change run.

## Method
- **Drivers:** the S13 harness functions, unchanged. `drive.cpu` gives R5, and `drive.f5` gives F5, both called through a focused runner (`target/s14b/focus.py`, not committed). That runner uses the harness's own session, isolation, guard, co-tenant watch and `gate.check`, and it runs only the R5 and F5 phases. The counted proof is still one full `scripts/acceptance.sh s14b-1` on a quiet box.
- **R5:** 30 samples × 2 s while paused, without `--status`. PASS when the median is ≤ 1 %. A per-thread `/proc/<pid>/task/*/stat` breakdown over 60 s paused is recorded alongside.
- **F5:** 10 reps per focused run. Medians ≤ 500 ms; pause and resume ≤ 500 ms on every rep (gate rule). Every row must match (engine, seq) with the effect.
- **Before/after:** run base and after binaries interleaved (A/B/A) in the same hour, so that network drift shows up as drift and not as a fix.
- **Floor:** if seek or prev still exceed 500 ms after the engine fixes, report the median and p90 over ≥ 10 reps. Also report the split between MusicKit's own work (timers, its own requests in sequence) and ours (dispatch time, snapshot delay). Nate takes the threshold question.

## Baseline (base binary, b059db7, clean tree)
- R5: median 1.8 % (FAIL). The player thread is 1.45 % at ~195 wakeups/s, the 5 ms tick clamp. The only JS timer pending while paused is ~12 min out, and the audio threads are idle (corked).
- F5 (10 reps, today's network): pause 19, seek 789, resume 39, next 540, prev 802 ms (FAIL).

## Named false positives
- **Network drift between before and after.** Guarded by the A/B/A interleave.
- **Acknowledging before the effect.** `applied` must still rise only after the command's promise settles. A snapshot taken earlier must not carry the new `applied`.
- **A dirty tree.** `build.json` records it, and a dirty run doesn't count.
- **A harness that ran nothing.** The baseline FAILs above show both drivers can fail.
- **R5 measured while not Paused.** The gate requires Paused before and after the window.
