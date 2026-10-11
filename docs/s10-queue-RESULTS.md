# S10 queue panel: results (2026-10-11)

Pre-registration: `docs/s10-queue-PREREG.md`, with amendments 1–4. All four amendments are procedure fixes; no bar moved. Live harness: `scripts/app/s10-session.py`, under `sg render`.

**Environment.** Every run used weston headless `--renderer=gl` on the RX 5700 XT (radeonsi), a PipeWire null sink, the sink guard and an isolated config and keyring. The guard never killed the app. Every stream it saw was on the null sink. The default sink was unchanged after every session. The harness refuses to launch while another Bramley runs, because the instance slot is per user; it refused twice, while S8 was running.

## What shipped

- **Panel:** now playing, then up next, read from MusicKit's public queue. The window holds at most 50 items from the current one (id, title, duration), in memory only.
- **Jump:** a click sends `JumpTo{index, id}`. `player.js` calls `changeToMediaAtIndex` only if `queue.item(index)` still has that id.
- **Track menus:** "Play next" calls `playNext`, which splices at position+1. "Add to queue" calls `playLater`, which splices at `appendTargetIndex`. CLI and MCP `queue add` stay on `playLater`.
- **Remove: not shipped.** `Queue.remove` logs "has been deprecated" and throws for the current index, and `splice` is undocumented (rule 3).
- **Top-up:** once at most 10 items remain and the queue's tail is from the last batch, the next ≤ 100 loaded ids go out with `playLater`, paging in more if needed. It stops when:
  - the queue was replaced;
  - the list moved;
  - the queue reached 1,000 items.

## Live checks

| Run | Build | F8 | J (item 3) | T (top-up) | T-S (must FAIL) |
|---|---|---|---|---|---|
| live-1 | branch | FAIL (dropped seek) | PASS | not scored (98-item window) | — |
| live-2, live-3 | branch + `6b7b59d` | PASS | PASS | not scored (W measured after the top-up) | stalled at 96, not claimed |
| live-4 | branch + `6b7b59d` | FAIL (dropped seek) | PASS | VOID (MusicKit `setQueue` TypeError) | **FAIL as designed** |
| **live-5** | branch + `6b7b59d` | FAIL (dropped seek) | PASS | **PASS** | **FAIL as designed** |
| live-6 | branch | FAIL (dropped seek) | PASS | FAIL: 3 stalls at index 95 | stalled at 95 |
| **live-7** | branch, and branch + `6b7b59d` | **PASS on both** | **PASS on both** | — | — |

- **F8 (live-7):** `play A` then `queue add B, C, D`. `now.queue` listed `[A, B, C, D]` (position 0, length 4). The items seen were exactly A→B→C→D, each Playing. F8-S (the reordered comparator) is False, as required.
- **T (live-5):**
  - The 100-id window held 98 items; MusicKit drops unplayable ids.
  - At position 95 the top-up grew the queue to 197, and the songs pager went from 1 to 2 pages with no scroll.
  - Index 98, the first item after the window, was song 100, Playing.
- **T-S (live-4, live-5):** `queue add X` right after the window. The queue stayed at 99 (no top-up), and index 98 was X.
- **No names in files.** Every run's status file had no `queue`, `queuePosition` or `queueLength` key and no library name. No `now.queue` entry carried a title. The leak grep covered the diff, the commit messages and the app logs, and its planted-name control proved it can fail. The only hits were "Back" (`Nav::Back`) and "Amen" (inside "Amendment"). The harness's stderr hit lists were deleted after reading.
- **Memory (dev build, report only):** PSS 179.5 MiB with the 98-item window, and 179.5 after the top-up to 197.

**Why the earlier runs failed (amendments 1–4).** None of these was a fault in queue logic:
- A 100-id window is 98 items.
- The top-up lands before the harness's next poll.
- **Dropped seek.** The F8 harness seeked C the instant MusicKit reported it, while C was still loading. Every failing run ended at C's start + 19 s on the native clock: the seek never took, and C outlasted the 30 s window.
- **Controls.** 12 runs (6 on main, 6 on this branch, at two pacings) and a replay of the harness loop all played A→B→C→D.

**Open, not S10's:**
- **Index-95 stall.** On the branch-only build, T stalled at index 95 in both T and T-S. The `6b7b59d` build plays through it. That fits S8's short-last-frame hang. It is an inference, not a proof.
- **`setQueue` TypeError.** live-4's T was VOID: MusicKit's `setQueue` threw "cannot read property 'id' of undefined" on the 100-id window. This is the same click path S9 ships. It was seen once in 5 T attempts.

## Gate

| Run | Commit | R1 PSS / USS / RSS | R2 PSS / USS / RSS | Reference PSS | Ballast | Gate |
|---|---|---|---|---|---|---|
| s10-1 | c911ccd | — | — | missing | — | FAIL: no `target/reference/` in a fresh worktree |
| **s10-2** | **c911ccd (clean)** | **173.2 / 146.3 / 205.8** | **75.3 / 48.4 / 107.9** | 88.6 / 88.6 | 69 MiB, rise 70.5, holds | **PASS** (zone 3) |
| main-s10delta-2 | c794b70 (clean) | 172.9 / 146.0 / 205.5 | 75.1 / 48.2 / 107.7 | 88.7 / 88.7 | 69 MiB, rise 70.5, holds | PASS |

- **Delta vs main** (same session, lead over the reference): R1 +0.4 MiB, R2 +0.3 MiB. That is within noise.
- **R1 headroom:** 26.8 MiB.
- **Shipped guard:** `check-shipped-features.sh` exits 0 on the release binary in both runs, and 1 on the ballast binary.

## Tests and mutation

- **Engine:**
  - `the_snapshot_carries_the_queue_window_from_the_current_item` (stub engine);
  - `the_queue_window_reads_the_public_queue_and_jumps_only_to_the_item_shown` (player.js on a fake queue: playNext versus playLater order, the window cap, the stale-jump guard);
  - the id-escaping test now covers `playNext` and `jumpTo`.
- **App:**
  - `queue_actions_send_play_next_play_later_and_jump_in_order_once_playing`;
  - `a_queue_row_jumps_to_its_absolute_index_and_the_current_row_does_nothing`;
  - `a_songs_window_tops_up_once_near_its_end_paging_in_what_it_needs`;
  - `a_window_stops_following_a_list_that_was_replaced`;
  - the status-file test, which now carries a queue.
- **Mutation: 21 of 21 killed.** One survivor (the tail-membership check) was killed after the test was strengthened. Two MEMORY_ONLY mutants first "died" on a compile error; they were re-run with the array size fixed and are killed by the test.

## Screenshots

- **Public:** `docs/screens/s10/queue-synthetic.png` (`--queue-fixture`, synthetic songs). The check counts distinct rows in the queue panel against a no-fixture control: 229 vs 51. The broken first build scored 51 vs 51 and failed.
- **Private (real art and names):** `target/screens/PRIVATE-real-art/s10/{f8,songs}.png` in the S10 worktree.

## Budget (plan cap 250 source / 120 tests / 0 fixtures)

- **Source:** +279 / −17. About 45 of those lines are the dev-only hooks the live proof needs (`--click-row`, `--jump-to`, `--queue-fixture`).
- **Tests:** +246 / −6, about 2× the cap. rustfmt line splitting is part of it, and the top-up rules each need a mutation-killing case.
- **Fixtures:** 0.
- **Harness:** `scripts/app/s10-session.py`, 232 lines, counts against S13's cap.

## Not established

- **A `queue add` after a `PlayIn` window lands after the window,** not after the current track; that is `playLater`'s semantics. It also ends the top-up, because the tail is no longer from the window. F8 holds because `play <id>` makes a one-song queue.
- **The 1,000-item cap is unmeasured at size.** One top-up of 99 items added no measurable PSS.
- **"Play next" and the context menus were not driven live.** They are covered by the unit and fake-queue tests only.
- **The jump on a stale row** (a position that shifted between render and click) is proven only against the fake queue.
