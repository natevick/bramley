# S22 soak trace run: pre-registration

Written before the run. Item ids appear in no committed file; records name items by duration. Probes 1 to 3 landed fast everywhere, so this is the pre-registered next step: trace a full R3-length session, where the slow seek is known to occur.

## Run
`acceptance.sh NAME --soak 30 --trace`. It is the usual soak (R3 session, sink, guard, pacing, leak grep, soak verdict) on a `--features trace` build, whose log is in the run dir and so under the leak grep. The shipped binary still passes check-shipped-features. The soak's own verdict is unchanged and is not a shipped-build measurement. A trace build logs every `currentTime` read, so its memory and timing are not the shipped build's.

## What is recorded
For every R3 seek, in `soak.r3.json` under `seekWindows`: status every 200 ms from just before the seek to 12 s after (seq, applied, skippedMs, sentMs, position, duration, state; no ids), and the trace between the seek and the end of the window, as relative milliseconds from the `seekToTime` start: fetches (hashed host, byte lengths), `remove` ranges, appends and throws, rejected settles, the `currentTime` set, CDM events (type only).

R3 itself is unchanged: same ticks, same samples, same seek rule and target. The polling runs in a thread started just before each seek, which waits 10 ms so the window has a pre-seek row. A test runs the driver with and without `--trace` and compares the commands. Not done: SourceBuffer stats at the seek mark. That needs a new engine op and is not cheap.

## Why s17 seeked LONG at 287 s and v012 at 145 s
R3's code is identical in both worktrees (diffed), and so is the position and buffer code: `git diff a08dca0 3cf4b35` over crates/engine and crates/app leaves runtime.rs and buffer.rs alone and has no position change, only S20 storefront and sign-in work. The seek fires when `pos - starts[at] >= 150`, where `starts[at]` is the position at the first tick that sees the new item. LONG's seek came about 290 s after its change in s17-counted-1, -2 and -3, and 150 s after in v012-counted-1 and s15c. One s17 run (soak-120) has both: 291.0, 150.0, 290.0. So `starts` was about 140 on some transitions and about 0 on others, with the same build. 140 is PRED's duration (139.88): on those transitions LONG's first tick still showed PRED's final position. This is a race at the seam, not a build difference. It hits only after PRED, the one predecessor R3 plays through. It is inferred from timing, since `starts` is not saved. Each window records the origin, which checks it. R3 seeks LONG at item position about 150, not 143 to 145, as 144.7 is a 10 s sample taken up to 6 s before the seek.

## Verdicts, reported beside the soak verdict
- **CAPTURED:** at least one R3 seek with `t_app ≥ 1.5 s` (or never settled in 12 s) and a non-empty timeline in its window. MID (355.15 s) lands at about 1.75 s in 6 of 6 R3 seeks, so it is the expected capture. LONG at about 145 s is a bonus.
- **NOT CAPTURED:** no such seek.
- **VOID:** unclean session, or any of these, which are my addition because a harness fault must not read as NOT CAPTURED: no trace installed; no windows; a different number of windows than R3 seeks; a window without a seq, with under 48 of 60 polls, or with no `seekToTime` mark in its trace.
- The soak trace verdict is FAIL if the run is not safe (leak, real state, pacing, phase error). `acceptance.sh` exits on the soak gate, so read `soakTrace` in the printed brief.

## Step comparison, per captured seek
Steps and their fast-path interval from probe-2 and probe-3, in ms: debounce (first fetch or `remove`) 251, first fetch response 95, largest append 108, `currentTime` set 1, settle 26. Each is the largest interval seen on the fast path: debounce 251 to 251, response 320 to 346, append 387 to 461, `currentTime` 388 to 462, settle 414 to 477.

Steps are taken in order: debounce is the earliest `remove` or fetch request; the response is the first response after it; the append is the largest append after the response; the `currentTime` set is the first one after the append; settle is the `done` mark. For each seek the judge computes every step's interval from the previous step, subtracts the fast interval, and names the step that grew the most. A step that never arrives is charged the rest of the 12 s window. The waiting step is **identifiable** if it grew by 1000 ms or more. Throws are listed, not judged.

## Decision table
| Verdict | Next step |
|---|---|
| CAPTURED, waiting step identifiable | design the fix from that step |
| CAPTURED, no identifiable step | S22's investigation stops; the ship-or-hold question goes back to Nate with everything measured |
| NOT CAPTURED | the same |
| VOID | re-run once |
