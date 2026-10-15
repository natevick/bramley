# S22 slow-seek probe v3: pre-registration

Written before the first run. Item ids are runtime arguments and appear in no committed file; records use the labels SHORT, PRED, LONG. Read `s22-seek-probe-RESULTS.md` first.

This is a test of a specific hypothesis that came out of the probe-2 trace. It is not blind probing, and the stop rule from probe-1 and probe-2 is not in play.

## Hypothesis
MusicKit calls `remove(0, item-relative target)`, but MSE applies it in element time. In probe-2 LONG's timestamp offset was 139.9 s, so the remove cleared the playhead and the forward buffer and the target segment had room. In R3, SHORT is seeked to `dur-10` about 148 s in, then PRED plays through, so LONG starts at offset 383.7 s. The remove then misses the current item, the forward buffer stays, and the 4 MiB source-buffer quota (buffer.rs:85) can make the target append throw `QuotaExceededError`. MusicKit has to wait or retry, which is the 4.5 s stall: `skippedMs` flat, `sentMs` at wall rate.

**Prediction: REPRODUCED.**

## Sequence
Counted license actions: plays, seeks to `dur-10`, and item changes (two natural ones, plus any seen inside arm A). Expected 5 or 6. **Cap: 8**, refused in the runner and failed by the gate.
1. Play SHORT fresh (abort unless 243.86 ± 0.5). `queue add` the album tail from PRED onward, as R3 does.
2. Wait for SHORT's position to reach 145, then seek SHORT to `round(dur-10, 1)`.
3. PRED starts by itself and plays through. LONG starts by itself (abort unless its duration is 500.79 ± 0.5). Wait for LONG position 143 from natural play.
4. **Arm A:** seek to `round(dur-10, 1)`. Status every 200 ms from 2 s before the seek to 12 s after (expected polls: 70).

There are no F or C arms: probe-1 and probe-2 already give small-offset fast cases (n=3). The run uses the `--features trace` build; its log is in the run dir, so the leak grep covers it.

## Thresholds
**A shows the defect:** probe-1's thresholds verbatim: `t_skip ≥ 3.0 s` AND `0 ≤ t_app − t_skip ≤ 1.0 s` AND slope 0.9–1.1.

## Did it run (VOID if any fails)
- Unclean session, or any gate-safety failure.
- Under 80% of the expected polls.
- A does not count. A counts only if all of these hold:
  - itemId = LONG;
  - PRED was entered from SHORT and LONG from PRED, each by an item change with no command in between (the `seq` of each entry's first snapshot equals the `seq` of the SHORT seek's reply);
  - origin in [138, 160];
  - seq delta 1;
  - the trace has a `currentTime` set inside A's window equal to 874.5 ± 1.5. This checks that the offset was R3's 383.7 (383.7 + 490.8).

## Verdicts
VOID, else **REPRODUCED** if the defect is shown, else **NOT REPRODUCED**.

## Interpretations (mine)
- t_skip, t_app and slope are derived as in probe-1. Any `currentTime` set in the window may match.
- Polls are reads of the status file; the engine publishes every 250 ms.
- A trace build logs every `currentTime` read and may perturb timing. Not tested offline.

## Recorded, not judged
For A: relative timestamps (ms after the `seekToTime` call) of fetches (hashed host, byte lengths), `appendBuffer` calls and throws (error name only), rejected settles (error name only), every `remove` range, the `currentTime` set, and CDM events (type only).

## Decision table
| Verdict | Next step |
|---|---|
| REPRODUCED | read the trace and name the waiting step: a `QuotaExceededError` throw, a backoff, or something else |
| NOT REPRODUCED | the offset hypothesis is refuted; next, a full R3-length trace run |
| VOID | re-run once |
