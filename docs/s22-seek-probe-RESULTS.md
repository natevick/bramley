# S22 slow-seek probe: results

## s22-probe-1 (2026-10-14, commit 8204437): INCONCLUSIVE

Did it run: commit 8204437, clean tree, mode probe, real state unchanged, leak grep clean, 6 license actions (one replay), 71 polls in every arm, control arm C landed in 0.6 s.

| Arm | Origin | t_skip | t_app |
|---|---|---|---|
| A, LONG from about 150 s | 151.8 | 0.4 | 0.4 |
| B, LONG from about 295 s | 297.0 | 0.4 | 0.4 |
| C, SHORT control | 151.8 | 0.6 | 0.6 |

Arm A landed fast, so it does not count, and by the PREREG the verdict is INCONCLUSIVE. The probe did not exercise the slow path.

## Why the design missed it

Item labels: LONG is 500.79 s, MID is 355.15 s, and PRED is the 139.88 s item that comes before LONG in the acceptance playlist.

I tabulated every R3 seek in the post-S17 runs (s17-counted-1, s17-counted-2, s17-counted-3, s17-soak-30, s17-soak-120, v012-counted-1): 77 seeks, with landing measured from item-relative position. The script was scratchpad seek_table.py, which is not committed.

| Item | How it was entered | Origin | Gap to target | Landing | n |
|---|---|---|---|---|---|
| LONG | seamless from PRED | about 143 | about 347 | 4.57 to 5.06 s | 4 of 4 |
| LONG | seamless from PRED | about 287 | about 204 | 0.25 to 0.5 s | 5 of 5 |
| LONG | fresh, single-song queue (probe-1) | 151.8 | 339 | 0.4 s | 1 of 1 |
| MID | seamless | about 147 | about 198 | 1.71 to 1.96 s | 6 of 6 |
| a 243.86 s item | seamless | 144.7 | 89 | 3.81 s | 1 of about 7 |

All other seeks landed in at most 0.5 s.

LONG at the same gap was slow 4 of 4 times when it was entered seamlessly from PRED, and fast 1 of 1 times when it was entered fresh. The single-song design left out the entry path, and maybe the presence of a next item as well. Gap alone does not explain the table: MID at a gap of about 198 s is consistently about 1.75 s, while LONG at a gap of about 204 s is fast.

## Deviation from the decision table

The PREREG said to re-run an INCONCLUSIVE once. I did not repeat the identical run. A and B both landed in 0.4 s, so the miss is in the design, not noise, and an identical re-run would most likely spend 6 license actions to repeat 0.4 s. The pre-registered re-run is folded into probe v2 as its fresh-entry control arm. v2 copies R3's context: LONG sits mid-queue after PRED and is entered by a natural transition. It runs on a trace build with the leaks below fixed, so a reproduction also shows what `seekToTime` waits on. v2 is pre-registered in its own file before it runs. Stop rule: if v2 also fails to reproduce, blind probing stops, and the work moves to reading the trace from an R3-shaped run.

The trace build's leaks, found while building probe-1:
- `host()` keeps the query string or userinfo of a URL that has no path;
- the CDM trace prints key ids.

## s22-probe-2 (2026-10-14, commit 1115d66): INCONCLUSIVE

The run was valid: commit 1115d66, clean tree, mode probe2, real state unchanged, leak grep clean, 8 license actions, 71 polls in every arm. LONG was entered by a natural change from PRED (seq at entry equals seq of the last queue add, 13).

| Arm | Entry | Origin | t_skip | t_app |
|---|---|---|---|---|
| A | seamless from PRED, which played through | 144.9 | 0.6 | 0.6 |
| F | fresh | 151.8 | 0.6 | 0.6 |
| C | SHORT control | 151.8 | 0.8 | 0.8 |

Arm A counted, but the seek was fast in that context too. By the pre-registered stop rule, blind probing ends here.

### What the trace shows on the fast path

Times are in milliseconds after `seekToTime` starts. This is the first trace of a seek.

| Step | Arm A | Arm F |
|---|---|---|
| range fetch issued, then `remove(0, 490.8)` | 251 | 251 |
| 970 B response appended | 320 | 346 |
| 387 KB segment appended | 387 | 454 |
| `currentTime` set | 388 (630.73) | 455 (490.8) |
| `seekToTime` settles | 414 | 459 |

The fast path is a 250 ms debounce, then two sequential range fetches, then the `currentTime` set.

In arm A, `currentTime` is element time: 630.73 is LONG's timestamp offset of 139.93 plus 490.8. The `remove` end, 490.8, is the item-relative target.

### Hypothesis that came out of the trace (not yet tested)

The `remove` call uses the item-relative target, but MSE applies it in element time.

**In probe-2, the remove clears the playhead.** LONG's offset was only 139.9, so the remove covered element 0 to 490.8. That is everything through item time 350.9: the playhead and the whole forward buffer. The target segment then had room to append.

**In R3, the remove clears nothing relevant.** By the time R3 reaches LONG, about 19 minutes into the soak, the offset is far larger, so the remove misses the current item entirely. The forward buffer stays. `make_room` evicts only frames that end before playhead − margin (buffer.rs:85), and the quota is 4 MiB (buffer.rs:8). So appending the target segment can fail with a quota error, and MusicKit has to wait or retry.

Browsers give audio a larger quota. (Correction, later the same day: Chromium does not collect garbage ahead of the playhead in this state; see the last section.)

Two cases this does not yet explain:
- the fast R3 seeks of LONG from about 287 s;
- MID's steady ~1.75 s.

The next step is a trace from an R3-shaped run, looking for an `appendBuffer` throw named `QuotaExceededError` inside a slow seek window.

## s22-probe-3 (2026-10-14, commit 05f9fec): NOT REPRODUCED

The run was valid: commit 05f9fec, clean tree, mode probe3, real state unchanged, leak grep clean, 6 license actions, 71 polls. The chain matched R3: SHORT, then PRED, then LONG, with each change natural (seq 15 at both entries, equal to the SHORT seek's reply). The trace's `currentTime` set was 874.64, so LONG's offset was 383.84, R3's offset.

Arm A, LONG from origin 144.9: t_skip 0.6 s, t_app 0.6 s, slope 0.77. The trace has no throw and no rejected settle. The steps were:
- `remove(0, 490.8)` at 251 ms, then a second `remove(0, 490)` at 330 ms;
- the 387 KB append succeeded at 461 ms;
- `currentTime` was set to 874.64 at 462 ms;
- the seek settled at 477 ms.

The offset/quota hypothesis is refuted. R3's exact chain at R3's exact offset lands fast in a fresh session. By the PREREG, the next step is a full R3-length trace run.

## Where this leaves S22

The slow seek reproduces only inside a full R3 session, about 19 minutes in. Three short probes landed fast at every offset tried: 139.9, 383.8, and fresh. What has been ruled out:
- fresh entry versus seamless entry;
- PRED seeked versus PRED played through;
- the timestamp offset and the `remove` range;
- a quota throw on the seek path.

Still open:
- in R3, LONG seeked about 145 s in was slow 4 of 4 times, while about 287 s in was fast 5 of 5;
- MID is a steady ~1.75 s in R3;
- whatever state builds up over a session: earlier items, refills, retired media, CDM sessions, and the UI and cover load.

## s22-soak-trace-1 (2026-10-14, commit 2c4df59): CAPTURED

The run was valid: commit 2c4df59, clean tree, a 30-minute soak on the trace build, real state unchanged, leak grep clean, pacing OK (12 actions), and 9 windows for 9 R3 seeks.

The soak's own verdict was FAIL, and it does not bear on S22:
- 12 item changes, under the bar of 30. A 30-minute soak can't reach 30, as known.
- 1 output underflow and 1 readiness dip. This is the trace build, so these are not shipped-build measurements.

Three seeks were captured (t_app ≥ 1.5 s): the 223 s item at 1.79 s, the 172.9 s item at 2.79 s, and MID at 2.19 s. The race gave LONG an origin of 290.8, and it landed in 0.59 s, so LONG at about 150 s was not exercised. The other five seeks landed in 0.59 s; the 243.9 s item took 1.39 s.

### The judge's step attribution is wrong

The judge names "append" as the waiting step in all three captures, because its "append" step came back None and a missing step is charged the rest of the window. Yet each raw timeline has the segment append right after the response. Its "identifiable" is therefore an artifact. The step was identified by reading the raw timelines below.

### The raw timelines

Times are in ms after `seekToTime`. The fast path from the probes takes about 70 to 110 ms per range fetch.

| Item | Range fetch 1 (970 B) | Range fetch 2 (segment) | Other |
|---|---|---|---|
| 223 s | 751 (253 to 1004) | 521 (1004 to 1525) | none |
| 172.9 s | 1174 (253 to 1427) | 1220 (1427 to 2647) | the next item's license POST at 561 overlaps both fetches |
| MID 355 s | 147 | 729 | the append threw `QuotaExceededError`; after about 700 ms MusicKit removed (0, 340), re-fetched in 58 ms and appended successfully, then set `currentTime` |

**Waiting step: the two sequential range fetches to the audio CDN.** In the soak they take 0.5 to 1.2 s each, against about 0.1 s in the probes. On one seek there was also a quota-error retry of about 0.7 s.

The quota error happens because the remove range is item-relative while MSE applies it in element time (MID's offset is 203.5). It frees nothing, and the forward buffer fills the 4 MiB quota. This mechanism is real, but it was only the smaller share of one seek here.

Open question, which sets the fix: why are the range fetches 5 to 10 times slower inside the soak than in the probes? Candidates:
- queueing in our network layer behind cover or forward-fill downloads, or connection handling;
- real CDN variance.

### Why the range fetches were slow (investigator, 2026-10-14)

Our network layer is not the bottleneck. These are verified from code and logs:
- **No queue or cap:** every JS fetch gets its own thread, and ureq 3.4.2 HTTP/1.1 keep-alive pools them, so no fetch queues behind another (net.rs:85-95, 139-162). Covers use a separate agent, and all 50 had loaded by t=14 s.
- **No link to concurrency:** across all 699 soak fetches, duration does not correlate with the number of fetches in flight (Spearman ρ −0.00 on the audio host). The probes show the same: 0.00 and −0.02.
- **Slow in bursts:** the slowness came in wall-clock minutes. The audio host's per-minute median was 341–1239 ms in minutes 1–2 and 7–16, and 29–62 ms in minutes 19–26.
- **Every Apple endpoint slowed together:** the license POST went from 187 to 2503 ms and the activity POST from 337 to 1228 ms.
- **The cost is per request, not bandwidth:** a 970 B fetch took longer than the 338 KB fetch after it.

Ranked causes (inferred): latency on the local link or upstream, then Apple CDN variance. This part of the slow seek is not a Bramley defect.

### What S22 fixes

The quota retry (MID: about 0.7 s) is ours. MusicKit's `remove` frees nothing when the item's timestamp offset is non-zero, and `make_room` (buffer.rs:85) evicts only data behind the playhead, so a far-ahead target append throws `QuotaExceededError`.

The fix makes eviction match Chromium's MSE garbage collection. Its pass condition is pre-registered before a re-run.

LONG at about 150 s, the steady ~5 s case, was not captured: the R3 race gave it origin 290. Whether the quota retry is the whole of that case is unproven.

### The quota fix tried, and why it is parked (2026-10-14)

The fix was tried on `natevick/s22-eviction` at 790d646. It raises the quota to Chromium's 12 MiB desktop audio limit (`media/base/demuxer_memory_limit.h:35-39`, at chromium fee3a7f3) and is not merged.

**Chromium would throw in this state too.** Its `GarbageCollectIfNeeded` (`media/filters/source_buffer_stream.cc:736-891`) frees nothing ahead of the playhead here:
- it keeps the last-appended GOP;
- its back pass stops rather than create gaps;
- its pending-seek branch never fires, because MusicKit sets `currentTime` only after the append succeeds.

The earlier claim that browsers collect garbage ahead of the playhead was wrong.

**MusicKit fills to whatever quota exists.** The soak logged 239 `QuotaExceededError` throws in 30 minutes, at a median gap of 9.8 s; the probes logged 70 and 59, starting about 2 s in. A larger quota moves the cliff instead of removing it, adds an unmeasured +8–16 MiB per session, and its test only covers the 4 MiB-era state.

**Throws do not track slow seeks.** In the seek windows across the soak and the probes:
- throws occurred in fast seeks too: LONG from 290 at 372 ms, probe-3 A at 462 ms, and four other soak seeks;
- the missing forward-range `remove` does not separate slow from fast: MID was slow, while LONG from 290 and probe-3 A were fast.

Only MID's seek visibly waited on its own retry (about 0.7 s), so no eviction heuristic would target the slow seeks cleanly.

## Where S22 stands

**Explained:**
- the 1.4–2.8 s seeks are network-path bursts, not Bramley;
- one 0.7 s quota retry is MusicKit behaviour that Chromium likely shares.

**Not explained:** the steady ~5 s seek of LONG from about 150 s. It was never captured with a trace, because R3's PRED-to-LONG seam race decides whether LONG is seeked at 150 or at 290.

**Next step, if wanted:** make that race deterministic, then run one more traced soak.
