# S22 slow-seek probe: pre-registration

Written before the first live run. Item ids are runtime arguments and appear in no committed file; arms and records use the labels LONG and SHORT.

## Hypothesis
On the long item (duration 500.79 s), a seek to `dur-10` from about 150 s stalls upstream of the engine for about 4.5 s. MusicKit's `seekToTime` does not set `currentTime` until the stall ends. Meanwhile the engine plays stale pre-seek audio (`sentMs` at wall rate, `skippedMs` flat) and the overlay shows the target until `applied >= seq`. The stall depends on the origin: the same seek from about 295 s lands fast. Short items land in 0.2-0.5 s.

## Sequence
Single-song queues. Counted license actions: 2 plays, 3 seeks to `dur-10`, and one replay only if the long item ended before the seek back. Cap 6, refused in the runner and failed by the gate.
1. Play LONG. Abort before any seek if duration is not 500.79 +/- 0.5. Wait for position >= 150 from natural play.
2. Arm A: seek to `round(dur-10, 1)`.
3. Seek to 280. Wait for it to settle and for position >= 295 (15 s of natural play).
4. Arm B: seek to `round(dur-10, 1)`. Origin is about 295.
5. Play SHORT (duration < 300 or abort). Wait for position >= 150, or `dur-40` if smaller.
6. Arm C: seek to `round(dur-10, 1)`.

Each arm records status every 200 ms from 2 s before the seek to 12 s after: t, position, applied, seq, skippedMs, sentMs, itemId, duration, state. Nothing else is issued inside a window. Expected polls per arm: 70.

## Derived per arm (recomputed by the judge from the raw polls)
- `t_skip`: first poll at or after the seek with `skippedMs` above its last pre-seek value.
- `t_app`: first poll at or after the seek with `applied >= seq` of the seek's reply.
- slope: `sentMs` rise over wall time across polls in `[0, t_skip)`, or across the whole post-seek window if `skippedMs` never rises.
- origin: position of the last pre-seek poll. seq delta: reply seq minus the last pre-seek seq.
- None means the event did not happen in the 12 s window. A None slope means fewer than two polls in its range.

## Thresholds
- **A shows the defect (stall upstream of the engine):** `t_skip ≥ 3.0 s` AND `0 ≤ t_app − t_skip ≤ 1.0 s` AND slope 0.9–1.1.
- **A refutes the hypothesis:** `t_skip ≤ 0.5 s` with `t_app ≥ 3 s` (engine waiting on data/key), or slope < 0.5 (silence, not stale audio).
- **Origin effect:** B `t_app ≤ 1.0 s` supports it. B `t_app ≥ 3.0 s` refutes it.

## Did it run
- A counts only if itemId = LONG, duration 500.79 ± 0.5, origin in [145, 160], seq delta exactly 1, and `t_app ≥ 2.0 s`. If A lands fast, verdict INCONCLUSIVE, never PASS.
- C must have `t_skip` and `t_app` both ≤ 1.0 s, else VOID.
- Every arm needs ≥ 80% of expected polls, else VOID.

## Interpretations and additions (mine, not in the thresholds above)
- B never settling in the window counts as `t_app ≥ 3.0 s`. B between 1.0 and 3.0 s leaves the origin effect unresolved.
- Expected polls are 70 per arm, so the bar is 56. A poll counts only if its state is readable. It is a read of the status file, not a fresh snapshot: the engine publishes every 250 ms.
- A missing arm is VOID. A gate failure (leak, real state changed, pacing, phase error) or more than 6 license actions fails the gate.
- A clean session is also required: null sink only, default sink unchanged, no co-tenant GPU client.
- B's itemId and origin and C's itemId are recorded, not gated.
- A replay of LONG (6th action) happens only if it had ended before the seek back. `probe.json` records it.

## Verdict order
VOID, then INCONCLUSIVE (A does not count), then REFUTED (any refute branch above, including B), then PASS (defect shown and origin supported). Anything else is INCONCLUSIVE. Two rows the table below does not name are INCONCLUSIVE: A counted but neither shown nor refuted, and defect shown with the origin effect unresolved.

## Decision table
| Verdict | Meaning | Next step |
|---|---|---|
| PASS | defect shown and origin supported | the fix targets MusicKit's buffering gap; next step: trace |
| REFUTED | A refutes, or B refutes the origin effect | re-diagnose |
| INCONCLUSIVE | A did not count, or a row above is unresolved | re-run once, then stop |
| VOID | control, polls or session failed | re-run once, then stop |

## Not wired
The `trace` build is not used by this probe. Its host logger can print a path-less URL's query string, and the engine's CDM trace prints key ids.
