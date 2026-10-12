# S14b engine latency and paused CPU: results (2026-10-12)

Pre-registration: `docs/s14b-engine-perf-PREREG.md`, with Amendments 1–2.
- **Base:** b059db7 (S13).
- **Code commits:** abf2b62, f8916d5, e11055a.
- **Final code:** e11055a. edcc4b7 is docs only.

## Verdict
- **R5 PASSes**, measured the new way.
- **F5 PASSes on the effect for every control**, but only with pooled connections.
- **Pooling exposes a MusicKit preload race:** a permanent stall at an item preload in 2 of 7 pooled soaks. Without pooling, seek FAILs F5 (median 523 ms).
  - **This needs Nate's call.**
- **Full acceptance:**
  - `s14b-1` FAILed on R3 because of an engine race, fixed in f8916d5.
  - `s14b-2` was stopped mid-soak on the coordinator's instruction, so it does not count.

## R5: paused CPU (new rule: 10 s bins over 5 min, process plus child ticks, procs == 1)

| Binary | Median | Mean | Notes |
|---|---|---|---|
| base | 1.8 % | 1.79 % | Co-tenant present for ~2 of the 5 min. A clean old-rule base run also gave 1.8 %. |
| e11055a | **0.6 %** | 0.58 % | Quiet box. Bins range 0.5–0.7. |

**Cause and fix:**
- `Engine::tick()` clamped its wait to 5 ms, so the player thread made ~195 wakeups/s.
- While paused, MusicKit's only timer is ~12 min out, and the Pulse stream is already corked.
- The tick now sleeps until the next JS timer or Widevine timer unless something is live.
- Per thread, the player went from 1.45 % to 0.33 %, and from 195 to 5 wakeups/s.
- The UI thread, at 0.17 % and 13 wakeups/s, is app-side and untouched.

## F5 on the effect (n = 10 each, ms, median / p90; quiet box)
The effect rules come from the coordinator's S13 review. They were checked by polling `--status` every 5 ms, with `applied ≥ seq` as a second condition.
- **pause:** Paused, with position unchanged for 300 ms; the time reported is the first such poll.
- **seek:** Paused at exactly 120.
- **resume:** position past 120 and advancing > 0.05 s between polls.
- **next / prev:** the item changes ("item"); then position advances on the new item ("audible").

| Binary | pause | seek | resume | next item / audible | prev item / audible |
|---|---|---|---|---|---|
| base b059db7 | 26 / 41 | **530 / 783** | 51 / 55 | 281 / 525; 530 | **535 / 567**; 535 |
| e11055a, run 1 | 20 / 29 | 348 / 384 | 44 / 45 | 142 / 172; 428 | 193 / 246; 452 |
| e11055a without pooling | 25 / 60 | **524 / 560** | 50 / 57 | 269 / 310; 518 | 366 / 456; 578 |
| e11055a, run 2 | 29 / 40 | 351 / 384 | 50 / 51 | 152 / 197; 426 | 174 / 201; 425 |

**Engine changes:**
- A command's effect reached `--status` only at the next 250 ms snapshot, so base latencies sat on ~250 ms steps. The loop now snapshots when `applied` rises.
- Each fetch had used a fresh `ureq::Agent`, which meant a new TCP+TLS handshake every time. That costs ~95 ms fresh against ~30 ms reused. One pooled agent replaces it.

**Seek floor:**
- `seekToTime` arms MusicKit's own 250 ms `setTimeout` before its first range request. Our timers fire it on time, 0–1 ms late.
- After that comes ~60–80 ms of fetches, then a ~6 ms snapshot. No dispatch or GC over 5 ms was seen.
- Without pooling, seek sits at ~525 ms: the 250 ms MusicKit debounce plus two fresh-TLS fetches.

**Caveat for the effect rule:** our `position` counts frames handed to PulseAudio, not frames heard. "Advancing" therefore shows within ~50 ms of resume, before audio is actually audible, by up to the ~200 ms sink target. Base behaves the same way.

## R3: two engine races that faster fetches exposed, one MusicKit race left

| # | What | Whose | Status |
|---|---|---|---|
| 1 | A `SourceBuffer.remove()` queued from a media-event handler finished only after the next tick's network completions. A fast init segment then arrived while the buffer read `updating`, MusicKit never appended it, and the next segment decrypted against the previous track's init: decode errors, then a permanent stall. | ours | Fixed in f8916d5: queued tasks run after each completion and each media dispatch. Two regression tests. |
| 2 | MusicKit trims `[0, X)` 0.06–0.34 s past `currentTime` whenever the buffer is at quota. Our readiness ignored the ~1.5 s already queued in the sink, so each trim fired `waiting`, counted an underrun and showed Loading. Base had 30 of these in 10 min. A 10 s sampler phase-locks onto them. | ours | Fixed in e11055a: queued output counts toward readiness. Probe builds showed 0 spurious blips over ~16.6 min, against Amendment 1's bar of ≤ 2 per 10 min; base had 30 per 10 min. The two raw `waiting` events in that time are the real run-outs at race-3 stalls. Phase control: soak-after4 read Playing at every sample for 13 min. One regression test. |
| 3 | At a preload, MusicKit runs two loaders for the next item. When loader 2's playlist lands after loader 1's init append, neither loader calls the SourceBuffer again, and playback sits at Loading for good. There is no host exception, no unhandled rejection and no queue call of ours in that window. | MusicKit | **Open.** 2 of 7 pooled soaks after f8916d5 hit it (probeD, hunt-4), both at the same place: the track 2→3 preload at `ts_off 290.4168`, right after the 67 s track 2. That is the reproduction recipe, a 4.5 min soak. Traces: `target/s14b/hunt-4`, `soak-probeD`. |

**Race 3 attribution is not settled.** Pooled runs *without* f8916d5 (s14b-1, soak1, probeA, probeC) all passed this preload. Only one unpooled run *with* f8916d5 exists (soak-nopool), and it was clean. The data cannot separate pooling from f8916d5's task ordering, which follows browser task semantics. Separating them needs pooled runs without f8916d5 and more unpooled runs with it.

## Full acceptance runs

| Run | Commit | Result |
|---|---|---|
| s14b-1 | abf2b62 | **FAIL** on R3 (race 1) and F10 (s14a's Home recovery). PASS on F1–F9, R1, R2, R4 (4.0 %), R5 (0.5 %, old rule) and R6. F5 old rule: 19 / 371 / 59 / 180 / 241. Controls not run. |
| s14b-2 | edcc4b7 | **Stopped** during R3 when the coordinator said to stop running `acceptance.sh`. Teardown was clean: sink gone, default sink unchanged. Does not count. |

## Runs that don't count
- **Playback never started:** hunt-3, and the very first base R5 run. The base binary does this too.
- **dec2-nopool:** stopped after round 1 on `play: SERVER_ERROR`, a MusicKit error.
- **s14b-2:** partial, stopped as above.
- **Co-tenant overlap** with another agent's run (`probe-…` weston, s13 worktree), epoch windows:
  - eff1's F5 tail, ~1791819770–1791819823;
  - eff9's base R5, ~1791822060–1791822174.
  - Neither is used in the tables above, except eff9's base R5 row, which is flagged there.

## Not done
- No counted acceptance run on the final commit. It waits for the coordinator.
- Race 3 is MusicKit's. No workaround was added: no delays and no fetch serialization.
