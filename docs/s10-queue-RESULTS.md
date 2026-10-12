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
| live-1 | branch | FAIL: seek landed, then a freeze with 4 AAC decode errors (unexplained, seen once) | PASS | not scored (98-item window) | — |
| live-2, live-3 | branch + `6b7b59d` | PASS | PASS | not scored (W measured after the top-up) | stalled at 96, not claimed |
| live-4 | branch + `6b7b59d` | FAIL (dropped seek) | PASS | VOID (MusicKit `setQueue` TypeError) | **FAIL as designed** |
| **live-5** | branch + `6b7b59d` | FAIL (dropped seek) | PASS | **PASS** | **FAIL as designed** |
| live-6 | branch | FAIL (dropped seek) | PASS | FAIL: 3 stalls at index 95 | stalled at 95 |
| **live-7** | branch, and branch + `6b7b59d` | **PASS on both** | **PASS on both** | — | — |
| **live-8** (post-registration) | branch + `6b7b59d`, largest playlist | — | — | **PASS** | **FAIL as designed** |

- **F8 (live-7):** `play A` then `queue add B, C, D`. `now.queue` listed `[A, B, C, D]` (position 0, length 4). The items seen were exactly A→B→C→D, each Playing. F8-S (the reordered comparator) is False, as required.
- **T (live-5):**
  - The 100-id window held 98 items; MusicKit drops unplayable ids.
  - At position 95 the top-up grew the queue to 197, and the songs pager went from 1 to 2 pages with no scroll.
  - Index 98, the first item after the window, was song 100, Playing.
- **T on a playlist (live-8):** the 100-id window held 100 items. The top-up grew the queue to 200, and the tracks pager went from 1 to 2 pages with no scroll. Index 100 was track 100, Playing.
- **T-S (live-4, live-5, live-8):** `queue add X` right after the window. The queue stayed one longer than the window (no top-up), and the item at the window's end was X.
- **No names in files.** Every run's status file had no `queue`, `queuePosition` or `queueLength` key and no library name. No `now.queue` entry carried a title. The leak grep covered the diff, the commit messages and the app logs, and its planted-name control proved it can fail. The only hits were "Back" (`Nav::Back`) and "Amen" (inside "Amendment"). The harness's stderr hit lists were deleted after reading.
- **Memory (dev build, report only):** PSS 179.5 MiB with the 98-item window, and 179.5 after the top-up to 197.

**Why the earlier runs failed (amendments 1–5; amendment 5 is an erratum).**
- **Harness faults:**
  - A 100-id window of songs held 98 items.
  - The top-up lands before the harness's next poll.
  - **Dropped seek:** in live-4, 5 and 6, F8 seeked C the instant MusicKit reported it, while C was still loading. Each run ended at C's start + 19 s on the native clock, with 0 decode errors. The seek never took, and C outlasted the 30 s window.
- **Controls:** 12 runs (6 on main, 6 on this branch, at two pacings) and a replay of the harness loop all played A→B→C→D.
- **Unexplained, on the branch build:**
  - **live-1:** the seek landed (62.7 s, item time), then playback froze with 4 AAC decode errors. Not seen again.
  - Amendment 1 had wrongly blamed every F8 failure on S8's hang.

**Open:**
- **Index-95 stall.** On the branch-only build, T stalled at index 95 in both T and T-S. The `6b7b59d` build plays through it. That fits S8's short-last-frame hang. It is an inference, not a proof.
- **`setQueue` TypeError (unattributed, 1 of 6 T attempts).** live-4's T was VOID: `setQueue` threw "cannot read property 'id' of undefined" on the 100-id window. There is no control, because main can't drive a click from row 0, so S10 is not ruled out.

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
- **Harness:** `scripts/app/s10-session.py`, 242 lines, counts against S13's cap.

## Not established

- **T has passed only with `6b7b59d` cherry-picked** (live-5 on Songs, live-8 on a playlist). On the branch-only build it stalls at index 95. Re-run T once S8 merges.

- **A `queue add` after a `PlayIn` window lands after the window,** not after the current track; that is `playLater`'s semantics. It also ends the top-up, because the tail is no longer from the window. F8 holds because `play <id>` makes a one-song queue.
- **The 1,000-item cap is unmeasured at size.** One top-up of 99 items added no measurable PSS.
- **"Play next" and the context menus were not driven live.** They are covered by the unit and fake-queue tests only.
- **The jump on a stale row** (a position that shifted between render and click) is proven only against the fake queue.

## After review (rebased onto main 750bfc2, which carries the short-frame fix)

**Before the fixes, on the branch alone (live-9, f000025):**
- T PASS on Songs: 98 → 197, song 100 at W.
- T PASS on the largest playlist: 100 → 200.
- T-S FAILs as designed on both.

**The review's fixes (52fbf9b):**
- **B1:** `queue.items`, not the undocumented `queue.item`, with placeholder slots.
- **B2:** a queue generation; `reset()` and an album click clear the follow.
- **M1/H1:** 16-id batches, retried at 5, 30 and 60 s, then a visible notice.
- **M2:** `setQueue` waits, bounded, for an add in flight.
- **Lows:**
  - a hard cap;
  - top-ups as background commands (not pending, not delaying a pause);
  - position −1 handling;
  - a borrowed snapshot, and one `redact` per snapshot.

**Proof that the new tests can fail:**
- **Probes:** the review's P1a/b/c and P3 are now tests. They fail on the pre-fix code, by the review's own probe runs, and pass now.
- **Mutation:** 16 of 16 mutants of the new logic killed.

**Live-10 on the branch alone (52fbf9b):**

| Check | Build | Result |
|---|---|---|
| F8 | dev | **PASS**: A→B→C→D |
| J | dev | **PASS** |
| T, Songs | dev + engine diagnostics | **PASS**: W 98; song 100 at W; two top-ups landed (114, then 130); pages 2 |
| T, largest playlist | dev + engine diagnostics | **PASS**: W 100; track 100 at W; 116, then 132; pages 2 |
| T-S, both | dev + engine diagnostics | **FAIL as designed** (see below) |
| H1 counts | dev + engine diagnostics | `/v1/me/library/songs`: 200 × 8 in each T run. No `setQueue` TypeError occurred, so H1 stays open |

- **T-S detail:** the item at W was X, and the queue never grew. The harness then raised on its deliberate seek past the queue's end, so the run's T flag was not written.
- **Names stay out of files:** every status file was clean. Leak-grep hits: "Amen" (inside "Amendment"), "Back" (`Nav::Back`), "Close" (`ViewportCommand::Close`) and "windows". The planted-name control proved the grep can fail.

**Gate `s10-3` (1f7d886, clean): PASS in zone 3.**
- R1: 172.9 PSS / 145.9 USS / 205.5 RSS.
- R2: 75.5 / 48.5 / 108.0.
- Reference: 90.2 / 90.5. Ballast: 70 MiB, rise 71.5, holds.
- Shipped guard: 0 on the release binary, 1 on the ballast binary.
- **Against main** (`main-s10delta-2`, an earlier session), on the lead over the reference: R1 −1.5 MiB, R2 −1.1 MiB. That is noise.

**Lines this round:** source +173 / −59, tests +164 / −51; the asked budget was about 150 each. Total against main: source +401 / −25, tests +359 / −6. Harness: 250 lines.

## After S8 merged (rebased onto main 98db09a)

**The rebase:** conflicts were resolved as unions. S8's Shuffle command, its drain/skip snapshot fields, its detail views and its dev flags sit alongside S10's queue commands, fields and flags.
- **One test broke in the merge:** the escaping test sent `PlayLater` twice. Fixed in 9bcee7a.
- **Generation bump:** S8's detail rows emit `PlayIn` and `Shuffle`, and both go through `Playback::play()`, which bumps the top-up generation. A test now pins both bumps, and a mutant that skips the bump on Shuffle fails it.

**Checks:**
- tests pass: app 128, engine 105;
- clippy is clean on four feature sets;
- the shipped guard exits 0 on the release binary.

**Live-11 (9bcee7a, dev build, branch alone):**
- **F8 PASS:** A→B→C→D.
- **T on Songs PASS:** W 98, song 100 at W, two top-ups landed (114, then 130), pages 2.
- **T-S FAILs as designed:** X is at W and the queue never grew.

**Gate `s10-4` (9bcee7a, clean): PASS in zone 3.**
- R1: 173.7 PSS / 146.7 USS / 206.4 RSS.
- R2: 75.5 / 48.5 / 108.2.
- Reference: 89.3 / 89.1. Ballast: 69 MiB, rise 70.3, holds.
- Shipped guard: 0 on the release binary, 1 on the ballast binary.
