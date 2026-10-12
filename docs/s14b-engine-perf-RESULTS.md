# S14b engine latency and paused CPU: results (2026-10-12)

Pre-registration: `docs/s14b-engine-perf-PREREG.md`, with Amendments 1–3.
- **Base:** b059db7 (S13).
- **Code commits:** abf2b62, f8916d5, e11055a, and 5438da1 (HTTP cache).
- **Trace feature:** df89543 and its follow-up, not shipped.
- **Final code:** 5438da1.

## Verdict
- **R5 PASSes**, measured the new way.
- **F5 PASSes on the effect for every control.** It needs pooled connections; without them seek is 523 ms.
- **Race 3 (preload stall) was ours and is fixed in 5438da1.**
  - **Cause:** our fetch had no HTTP cache, so MusicKit's duplicate playlist request could land in a ~60 ms window that a browser's cache closes.
  - **Pre-registered bar met:** 0 stalls in 12 pooled 4.5 min soaks on the fix, against 4 in 16 before it.
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
| 3 | At a preload, MusicKit runs two loaders for the next item. When loader 2's playlist lands after loader 1's init append, neither loader calls the SourceBuffer again, and playback sits at Loading for good. There is no host exception, no unhandled rejection and no queue call of ours in that window. | ours (platform) | **Fixed in 5438da1.** See below. |

### Race 3: root cause

**What happens (trace build; MusicKit read only to explain it, never patched):**
1. At the item change MusicKit runs `prepareToPlay` twice: one trigger is the Preloader threshold, the other the queue change. Its `isPreparedToPlay` guard is only set once a preparation completes, so both run.
2. As a result it fetches the next playlist twice, from the same URL (`Cache-Control: public, max-age≈1 y`), 5–97 ms apart.
3. Preparation 1 does three things in order: `setNextManifest`; `transitionToNextManifest`, which sets `nextSeamlessTransition = 290.4168`; and init and media loading.
4. Preparation 2 then calls `setNextManifest`, which sees a pending transition and runs `revertSeamlessTransition` → `clearBufferToEnd(290.4168)` → `safeSourceBufferRemove(290.4168, buffered.end)`.
5. **The stall:** if preparation 2's playlist lands after the next track's init append but before its first media append, `buffered.end` is still 290.4168. The range is empty, so MusicKit only warns and never calls `remove()`. No `updateend` follows, so no transition and no loading. The media segment already in flight is dropped, because `segmentIndexToFetch` was reset to -1.
   - In tr-6 the trace shows `buffered` [209.676-290.417] and `updating false` read right after the second playlist resolves, then no SourceBuffer call ever again.
6. **When it survives:** if the playlist lands after a media append, the remove is valid and MusicKit recovers (tr-2).

**Diff of stalled against passing traces:** the classifier `target/s14b/window.py` asks where the second playlist resolves.

| Runs | Before the init append | Inside the init→media window | After a media append |
|---|---|---|---|
| 2 stalls (tr-6, tr-9) | – | 2 | – |
| 7 clean (tr-1…5, 7, 8) | 4 | 0 | 1 (tr-2); 2 runs had no duplicate |

**Where we deviate from a browser:**
- A browser's `fetch` for cache mode "default" goes through its HTTP cache: Fetch Standard, "HTTP-network-or-cache fetch", serving fresh stored responses per RFC 9111 §4 and §4.2.
- Chromium (the HTTP cache's writer lock) and Firefox (cache-entry sharing) also make a second identical in-flight GET wait for the first.
- So both playlists resolve together, before the init fetch has even started. MusicKit's revert then lands while the range is either empty and harmless, or never reached.
- We had no HTTP cache, so the duplicate kept its own network timing.
- Strictly, Fetch permits a user agent with no cache. The failure needs the browser behaviour that rule 2 asks host objects to have, not a step that is a MUST.
- Pooling exposed it: it shrank the init and media fetches to ~17–60 ms, against ~100 ms each with a fresh TLS handshake.

**Fix (5438da1):** an in-memory HTTP cache in `net`.
- **In scope:** GETs with cache mode "default", no body and no Range header.
- **Stored:** 200 responses, fresh by `max-age` less `Age`, keyed by URL plus the request headers named in `Vary`.
- **Not stored:** `no-store`, `no-cache`, `Vary: *`, and `Authorization` without `public`.
- **Coalescing:** a concurrent identical request waits for the first.
- **Limits:** 512 KiB per entry, 2 MiB in total.
- **Media segments are untouched:** they are 206 range requests.
- **Tests:** 3 new; the wiring test fails without the fix (3 network requests against 2).

**Did it run:** three traced soaks on the fix show the duplicate playlists resolving 2–3 ms apart, always before the init append. Before the fix they resolved 13–119 ms apart.

### Race 3: soak table (4.5 min pooled soaks; stall per Amendment 3)

| Build | Valid runs | Stalls | VOID |
|---|---|---|---|
| Before the fix: f8916d5 (+ e11055a for most), pooled, no cache. Runs: after3-1/2, after4, probeD, hunt-1/2/4, tr-1…9 | 16 | **4** (probeD, hunt-4, tr-6, tr-9) | 1 (hunt-3) |
| **Fix 5438da1, shipped binary (fix-1…13)** | **12** | **0** | 1 (fix-10: SignInNeeded at start) |
| Fix 5438da1, trace build (trfix-1…3) | 3 | 0 | 0 |
| Confound: pooled, no f8916d5, with e11055a, no cache (conf-1…8) | 8 | 0 | 0 |

**Confound:**
- Pooled runs without f8916d5 had 0 stalls in 12: conf-1…8, plus s14b-1, soak1, probeA and probeC earlier. Runs with it had 4 in 16.
- That difference is not significant (Fisher exact, one-sided p ≈ 0.09), and the mechanism above does not involve task ordering.
- f8916d5 stays: it fixes race 1 and matches the HTML event-loop order.
- I can't rule out that it widens race 3's window. The fix closes the window either way.

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
- **F5 spot check on the fix:** eff11-fix gave 20 / 376 / 45 / 164 (audible 430) / 163 (audible 408). It overlapped another agent's session that ran without the lock, so it is indicative only.
