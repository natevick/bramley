# S22 soak trace run: pre-registration

Written before the run. Item ids appear in no committed file; records name items by duration. Probes 1 to 3 landed fast everywhere, so this is the pre-registered next step: trace a full R3-length session, where the slow seek is known to occur.

## Run
`acceptance.sh NAME --soak 30 --trace`. It is the usual soak (R3 session, sink, guard, pacing, leak grep, soak verdict) on a `--features trace` build, whose log is in the run dir and so under the leak grep. The shipped binary still passes check-shipped-features. The soak's own verdict is unchanged and is not a shipped-build measurement. A trace build logs every `currentTime` read, so its memory and timing are not the shipped build's.

## What is recorded
For every R3 seek, in `soak.r3.json` under `seekWindows`: status every 200 ms from just before the seek to 12 s after (seq, applied, skippedMs, sentMs, position, duration, state; no ids), and the trace between the seek and the end of the window, as relative milliseconds from the `seekToTime` start: fetches (hashed host, byte lengths), `remove` ranges, appends and throws, rejected settles, the `currentTime` set, CDM events (type only).

R3 itself is unchanged: same ticks, same samples, same seek rule and target. The polling runs in a thread started just before each seek, which waits 10 ms so the window has a pre-seek row. A test runs the driver with and without `--trace` and compares the commands. Not done: SourceBuffer stats at the seek mark. That needs a new engine op and is not cheap.

## Why s17 seeked LONG at 287 s and v012 at 145 s
R3's code is identical in both worktrees (diffed). The seek fires when `pos - starts[at] >= 150`, where `starts[at]` is the position at the first tick that sees the new item. LONG's seek came 290 s and 291 s after its change in s17-counted-1 and -2, and 150 s after in v012-counted-1. So `starts` was about 140 in s17 and about -5 in v012. 140 is PRED's duration (139.88): in the s17 build LONG's first tick still showed PRED's final position. Items after a seeked predecessor fired at about 145 in all three runs. PRED is the one predecessor R3 plays through, as it is too short to seek. This is inferred from timing; `starts` is not saved. The traced soak records each window's origin, which checks it.

## Verdicts, reported beside the soak verdict
- **CAPTURED:** at least one R3 seek with `t_app ≥ 1.5 s` (or never settled in 12 s) and a non-empty timeline in its window. MID (355.15 s) lands at about 1.75 s in 6 of 6 R3 seeks, so it is the expected capture. LONG at about 145 s is a bonus.
- **NOT CAPTURED:** no such seek.
- **VOID:** unclean session, no trace installed, or no windows recorded.

## Step comparison, per captured seek
Steps and their fast-path interval from probe-2 and probe-3, in ms: debounce (first fetch or `remove`) 251, first fetch response 95, largest append 108, `currentTime` set 1, settle 26. Each is the largest interval seen on the fast path: debounce 251 to 251, response 320 to 346, append 387 to 461, `currentTime` 388 to 462, settle 414 to 477.

For each seek the judge computes every step's interval from the previous step, subtracts the fast interval, and names the step that grew the most. A step that never arrives is charged the rest of the 12 s window. The waiting step is **identifiable** if it grew by 1000 ms or more. Throws are listed, not judged.

## Decision table
| Verdict | Next step |
|---|---|
| CAPTURED, waiting step identifiable | design the fix from that step |
| CAPTURED, no identifiable step | S22's investigation stops; the ship-or-hold question goes back to Nate with everything measured |
| NOT CAPTURED | the same |
| VOID | re-run once |
