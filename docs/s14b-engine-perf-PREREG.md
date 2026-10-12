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

## Amendment 1 (2026-10-12, before any run of the readiness fix)
`s14b-1` (abf2b62) FAILed R3. Two engine causes were found. No bar changes.
- **Init-append race:** fixed in f8916d5, which runs queued tasks before the next network event.
- **Spurious `waiting` blips:** MusicKit trims `[0, X)` with X 0.06–0.34 s past `currentTime` whenever the SourceBuffer is at quota. Our readiness looked only at SourceBuffer ranges, so it reported `waiting` and counted an underrun while ~1.5 s of decoded audio was still queued in the sink.
  - Base has them too: 30 blips in 10 min, median 208 ms.
  - The harness samples every 10 s, and the blips repeat every 10 s, so whether a sample lands in one depends on phase.
  - The fix counts queued output toward readiness.

**Measures:**
- **Direct:** a probe build of the fix counts non-seek `["timeupdate","waiting"]` media events over a 10 min soak. PASS at ≤ 2, against 30 on base.
- **Phase control:** a 13 min soak on the shipped fix binary. PASS when it reads Playing at every sample. The window that matters is minutes 8.5–8.9 on track 4, where after3-2 hit three times.
- **Guard:** a genuine run-out still fires `waiting` (`running_out_of_data_mid_stream_fires_timeupdate_then_waiting`).
- **Counted run:** F5 (10 reps) and R5 are re-measured on the final commit, then the full acceptance runs as `s14b-2`.
