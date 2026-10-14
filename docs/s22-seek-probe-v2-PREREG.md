# S22 slow-seek probe v2: pre-registration

Written before the first run. Item ids are runtime arguments and appear in no committed file; records use the labels PRED, LONG, SHORT. Read `s22-seek-probe-RESULTS.md` first: probe-1 was INCONCLUSIVE because a single-song queue never reproduced the slow path.

## Hypothesis
In every slow R3 seek, LONG (500.79 s) was entered by a natural seamless transition from PRED (139.88 s), its predecessor in the queue. A seek to `dur-10` about 145 s into LONG then stalls upstream of the engine for about 4.5 s: `skippedMs` stays flat while `sentMs` rises at wall rate. The same seek after a fresh single-song entry lands in under 1 s. The trace build shows which step waits.

## Sequence
Counted license actions: plays, seeks to `dur-10`, and item changes (the natural PRED to LONG change, and any change seen inside arm A). Expected 7 or 8. **Cap: 10**, refused in the runner and failed by the gate.
1. Play PRED (abort unless 139.88 ± 0.5). Queue the rest of the acceptance album from LONG on, as R3 does (`queue add`; the CLI has no play-a-playlist-from-an-index).
2. Let PRED play through, as R3 does: R3 never seeks PRED. Wait for LONG to start by itself and reach position 143 (abort unless its duration is 500.79 ± 0.5).
3. **Arm A:** seek to `round(dur-10, 1)`.
4. **Arm F (fresh-entry control, probe-1's re-run folded in):** play LONG as a single-song queue, wait for position 150, seek to `round(dur-10, 1)`.
5. **Arm C (control):** play SHORT (abort unless duration < 300), wait for position 150 or `dur-40` if smaller, seek to `round(dur-10, 1)`.

Each arm records status every 200 ms from 2 s before the seek to 12 s after (expected polls: 70). The run uses a `--features trace` build whose log lives in the run dir, so the leak grep covers it.

## Thresholds
- **A reproduces:** probe-1's defect thresholds verbatim: `t_skip ≥ 3.0 s` AND `0 ≤ t_app − t_skip ≤ 1.0 s` AND slope 0.9–1.1.
- **Entry effect:** supported if F `t_app ≤ 1.0 s`, refuted if F `t_app ≥ 3.0 s`, unresolved otherwise.

## Did it run
- A counts only if: itemId = LONG; entered by an item change with no command in between; origin in [138, 160]; seq delta 1; the trace log has a seek-related event inside A's window.
- VOID if: the session is unclean, any arm has under 80% of its expected polls, or control C does not have `t_skip` and `t_app` both ≤ 1.0 s.

## Interpretations (mine)
- t_skip, t_app and slope are derived as in probe-1's PREREG. F never settling counts as `t_app ≥ 3.0 s`.
- "No command in between" means the `seq` of LONG's first snapshot equals the `seq` of the last `queue add` reply.
- A seek-related event is a `seekToTime` step mark or a `currentTime` set in the trace. A's duration must also be 500.79 ± 0.5.
- Polls are reads of the status file; the engine publishes every 250 ms. The leak grep, pacing and safe-flag rules are unchanged.
- R3 does not seek PRED (it is shorter than R3's 165 s bar); it plays PRED through, and so does this probe. This was amended before the first run; the first draft seeked PRED to save about 130 s, a context R3 never produced.
- A trace build logs every `currentTime` read and may perturb timing. That is not tested offline.

## Verdicts and decision table
Order: VOID, then INCONCLUSIVE (A does not count or does not reproduce), then REFUTED (entry effect refuted), then PASS (A reproduces and the entry effect is supported). Anything else is INCONCLUSIVE.

| Verdict | Next step |
|---|---|
| PASS | read the trace to name the step that waits |
| INCONCLUSIVE | stop probing blind |
| REFUTED | re-diagnose |
| VOID | re-run once |

## Recorded, not judged
For A and F: relative timestamps of the `seekToTime` call, fetches (hashed host, byte lengths), `appendBuffer` calls and throws (error name only), `remove`, the `currentTime` set, and CDM events (type only).
