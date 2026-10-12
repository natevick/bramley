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

## Amendment 2 (2026-10-12, before s14b-2)
- **Correcting Amendment 1's direct measure:** probeD's "0 blips" covers only ~3.8 min of normal playback before probeD stalled, not 10 min.
- **A second race:** probeD stalled for good at a preload, the track 2→3 transition.
  - Probe E (every SourceBuffer and MediaSource call, a rejection tracker, our queue traffic) reproduced it on hunt attempt 4 of 4. Attempt 3 is VOID: playback never started.
  - **Trace:** MusicKit starts two loaders for the next item, as base also does. Loader 1's init is appended. Loader 2's playlist response lands after that append, and from then on neither loader calls a SourceBuffer method.
  - **Not us:** no host-object exception, no unhandled rejection, and no queue call of ours inside that window.
  - **Classification:** a MusicKit-internal race. Pooled connections expose it because they let the init arrive ~17 ms after the request.
  - No workaround is added: no delays and no fetch serialization.
- **R3 in s14b-2:** if this race causes an R3 failure, it is reported as such. The run is not repeated until it goes green.
- **Decision data for Nate:**
  - F5 interleaved: after4 (pooled), then after4-nopool, then after4 again, 10 reps each.
  - One 13 min after4-nopool soak, as the control.

## Amendment 3 (2026-10-12, race 3 is a release blocker; written before any counted soak)
Race 3 is the permanent Loading stall at a preload with pooled connections. It is treated as ours until a spec-conformant platform is shown to stall too.
- **Trace:** a non-shipped `trace` feature, carrying the `bramley-unshipped-feature` marker. It logs every MSE, media, fetch and XHR call, getter, event and completion, with a monotonic timestamp and object identity. It logs hosts only: no URLs and no titles. The traces stay under the gitignored `target/`.
- **Diagnosis:** capture ≥ 2 stalls with the 4.5 min soak (`SOAK_S=270`). Diff each against a passing trace at the same preload (`ts_off 290.4168`), and name the spec step we deviate from.
- **Pass condition:** 0 stalls in ≥ 12 pooled 4.5 min soaks on the fix commit, using the shipped (non-trace) binary.
  - A stall is either of these:
    - the item stuck at Loading with position frozen for ≥ 20 s before the soak ends;
    - any position stall > 2 s (the R3 proxy) that is not explained by a seek.
  - VOID runs, where playback never started, are reported and not counted. Replacements are added until there are 12 valid runs.
  - At the observed ~2/7 rate, a no-op fix shows 0 in 12 with probability ≈ 1.6 %.
- **Confound:** report pooled soaks without f8916d5, that is abf2b62 with e11055a's readiness fix applied, at the same 4.5 min.
- **Box:** each soak holds the shared GPU-session flock (`/run/user/$UID/bramley-gpu-session.lock`), the same one S13's `acceptance.sh` takes. Each also gets a unique null sink and the routing guard.
