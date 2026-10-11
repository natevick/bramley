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
