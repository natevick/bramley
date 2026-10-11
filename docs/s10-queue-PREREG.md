# S10 queue panel: pre-registration (2026-10-11)

Written before any live run of the S10 build. Thresholds do not move after data comes in.

## What runs

`scripts/app/s10-session.py`, run under `sg render`. It uses weston headless `--renderer=gl` on the real GPU, a PipeWire null sink, and the sink guard from `covers-session.py`. Each run uses an isolated copy of the config dir and a throwaway keyring service. The binary is the `dev` build of the commit under test. The app is driven only through the S11 CLI (`play`, `queue add`, `seek`, `now`) and the dev-only flags `--route songs`, `--click-row <n>` and `--jump-to <n>`. `--jump-to` emits the same `Action::JumpTo` a click on a queue row emits.

The harness calls the documented catalog and library endpoints itself (compliance rule 4), with Bramley's User-Agent and no Origin (rule 5). It needs them for two things: four catalog song ids from the album of `6781024437`, and the ids and names of the first 200 library songs. Names go only to the gitignored `target/tmp/s10/names.txt` (0600), which is deleted at the end of the session, including on failure. Evidence files hold ids, counts, PSS and booleans; they never hold names.

## MusicKit semantics (read from musickit.js, sha256 `21908b72…012c`, before building)

- `playLater` → `appendToQueue`, which splices at `queue.appendTargetIndex`: the queue's end, or just before the first autoplay item.
- `playNext` → `prependToQueue`, which splices at `position + 1`. Three `playNext` calls therefore play in reverse; `queue add` stays on `playLater`.
- `changeToMediaAtIndex(i)` is public and changes the position.
- `Queue.remove` logs "The queue remove function has been deprecated" and throws for the current index. `splice` is not documented. S10 ships no remove control (rule 3).

## Checks and pass conditions

**F8. `queue add` ×3 plays in order after the current track**
- `bramley play A`, then `queue add B`, `queue add C`, `queue add D`, each exiting 0. A is `6781024437`; B, C and D are other tracks of its album.
- Within 20 s, `now.queue` lists `[A, B, C, D]` with `queuePosition` 0 and `queueLength` 4.
- For each of A, B and C: seek to its duration minus 5 s. Within 30 s, `itemId` becomes the next id, with state `Playing` and position > 0.
- The distinct `itemId`s observed (polled every 0.5 s) are exactly `[A, B, C, D]` in that order.
- The guard never killed the app, and it saw the stream on the null sink only.

**J. Jumping to queue item 3 (the plan's AC)**
- Same queue as F8, launched with `--jump-to 3`. Within 30 s of D being queued, `itemId` is D and `queuePosition` is 3, with state `Playing`.

**T. Top-up past a 100-song `PlayIn` window**
- Launched with `--route songs --click-row 0 --jump-to 98`. The clicked song (library id or catalog id of song 0) plays first.
- The library has ≥ 200 songs (precondition; otherwise T is VOID).
- Within 60 s of `queuePosition` reaching 98, `queueLength` is ≥ 200.
- `library.songs.pages` reaches 2 or more. The run never scrolls, so the second page comes from the top-up.
- Seeking near the end of items 98 and 99: within 30 s each, `queuePosition` is 100 and `itemId` is song 100's library or catalog id, with state `Playing` and position > 0.

**No names in files**
- At the end of every run, the status file has no `queue`, `queuePosition` or `queueLength` key, and none of the names in `names.txt`.
- Entries in `now.queue` carry no `title`.
- The leak grep covers the branch diff, the commit messages and every app log. It is proven able to fail by appending one planted name to a scratch copy of the diff.

**Did it run**
- radeonsi appears in every status, and frames are painted.
- The engine started in F8, J and T.

## Sabotage (each must FAIL its check)

- **T-S (top-up suppressed).** It is the same scenario as T, except that right after the window lands (`queueLength` 100), the harness runs `queue add` with a catalog song X. The queue's tail is then X, which is not in the window's batch, so the top-up never fires; that is a designed property. T must FAIL:
  - `queueLength` stays at 101 for 60 s at position 98;
  - item 100 is X, not song 100.
- **F8-S (comparator).** The F8 order check is re-scored against `[A, C, B, D]`, and it must FAIL.

## Report only (no bar)

- PSS (median of 10 samples) in T at `queueLength` 100, and again once it is ≥ 200.
- **Gate:** `sg render -c 'bash scripts/acceptance.sh s10-1'` must PASS as acceptance.md defines it. R1/R2 are reported against `main` gated in the same session from a separate worktree, using the lead over the reference binary.

## Named false positives

- The order check can pass if MusicKit skipped an item quickly. Every transition must also show `Playing` with position > 0 on the new id.
- `queueLength ≥ 200` alone could come from a double append. T also requires item 100 to be song 100, and the unit test pins one batch per landing.
- A `Playing` state with a catalog id for a library song is accepted; S9's L2 rule is to accept either id.

## Amendment 1 (after live-1, before any passing T or F8 run)

live-1 FAILed F8 and could not score T. A timeline of each (`now` polled every second; ids and counts only) found two causes. Neither is a queue-logic fault.

- **T's procedure assumed that a 100-id window gives a 100-item queue. It doesn't.**
  - MusicKit's `spliceQueueItems` drops unplayable ids. In live-1, the first 100 library songs gave `queueLength` 98, so `--jump-to 98` never fired (it waits for a length above 98).
  - **Changed procedure:**
    - Launch with `--jump-to 90` and record the window length W, which is `queueLength` at position 90. In T-S that is minus one, for X.
    - The top-up must grow the queue to at least W + 90: one batch of 100 ids, less the ones MusicKit filters. This replaces "≥ 200".
    - The harness seeks near each track's end until `queuePosition` is W. The item there, the first after the window, must be song 100 (library or catalog id), Playing with position > 0. This replaces "position 100 is song 100".
  - W ≤ 90 makes T VOID. So does an unplayable song 100.
  - T-S is unchanged in intent: the queue stays at W + 1 and the item at W is X.
- **F8 stalled near the end of track C.** The final snapshot read Playing at 62.7 s of a 67 s track, with AAC decode errors in the log at that moment. The diagnostic run went A→B→C→D in order. This is the engine hang S8 found: a last frame under 2 ms re-decoded forever, fixed in `6b7b59d` on `natevick/s8-detail`, not on main.
  - F8, J, T and T-S are run twice: on this branch's dev build, and on the same build with `6b7b59d` cherry-picked (throwaway worktree, never committed here).
  - Both results are reported, and the cherry-picked one is labelled.
  - The bars are unchanged.

## Amendment 2 (after live-2; no bar changed)

At position 90, a 98-item window has 7 items ahead, so the top-up fires at once and lands before the harness's next poll. live-2 therefore read W as 197. W is now the smallest `queueLength` seen between the click and position 90. In T-S it stays the length at position 90 minus one; T-S never tops up, and its live-2 length held at 99. live-2's other readings stand: F8 PASS and J PASS on the `6b7b59d` build. The top-up landed: the queue grew 98 → 197, songs pages 1 → 2.

## Amendment 3 (after live-3; no bar changed)

live-3 repeated live-2: F8 PASS and J PASS; the top-up landed (98 → 197, pages 2) and playback reached index 110. The harness still read W as 197, because it started collecting lengths after the 10 s PSS sample, by which time the jump and the top-up had both happened. Two procedure changes follow:

- **W** is now the smallest non-zero `queueLength` seen by any `now` poll since the engine started.
- **The jump target is 95**, which cuts the near-end seeks from 8 to 3. Each seek now waits for `Playing` first, and a seek that doesn't advance within 30 s counts as a stall. Up to 3 stalls are allowed and reported.

T-S in live-3 stalled at index 96 before reaching X, so it did not fail for the reason the PREREG names. It is not claimed.

## Amendment 4 (after live-6; no bar changed)

live-5 (with `6b7b59d`) PASSed T, J and T-S, which FAILed as designed. F8 stopped at C in live-1, 4, 5 and 6, but passed in live-2, live-3, all 12 control runs (6 on main, 6 on this branch, at two pacings) and a replay of the harness loop. In every failing run, the last position was C's start plus about 19 s on the native clock. A seek that lands resets that clock to item time, and the replay shows 62.2 → 67.8. So the seek on C never took. The harness sent it the instant C was first reported, while MusicKit was still loading it, and C, playing from its start, outlasts the 30 s window.

**Changed procedure:** before each seek, F8 waits 2 s and then for `Playing`. If no transition has come 15 s after a seek, it re-seeks once. The 30 s window and every bar are unchanged. live-6 also stalled three times at index 95 in T and T-S on the branch-only build: a track-end hang the `6b7b59d` build plays through. The T bar is judged on the `6b7b59d` build and reported on both builds.
