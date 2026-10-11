# S9 library lists: results (2026-10-11)

Pre-registration: `docs/s9-library-PREREG.md`, including amendment 1. Live harness: `scripts/app/s9-session.py`, run under `sg render`.

**Environment.** Every run used weston headless `--renderer=gl` on the AMD Radeon RX 5700 XT (radeonsi). Every run had the PipeWire null sink and the sink guard. Every run used an isolated copy of the config and keyring. The default sink was unchanged after every session, and the null sink was removed.

**Evidence.** It stays on the dev box under the S9 worktree, in `target/s9/` and `target/acceptance/s9-*`. It holds counts, PSS and booleans only. Library names exist only in the gitignored `target/tmp/s9/names.txt` (5,455 strings), which every commit's diff and message was grepped against. The grep was proven able to fail first: a scratch diff with one name appended scored 19 hits against 18. All the remaining hits are code words (`Route::Home`, `TimeoutExpired`, `cs.Guard`).

## Live checks, final: live-5 at 85bcfb3, PASS

| Check | Result |
|---|---|
| L1 largest playlist (250 tracks, 3 pages) | loaded 250 = total 250 = harness total; `shownTo` 250; rows built max **15**; textures 60 of 96. **PASS** |
| L2 click the last track | Playing, position advancing; the item playing is the clicked track; guard silent, null sink only. **PASS** |
| L3 Songs to the end (4,481, 45 pages) | loaded 4,481 = total = harness total; `shownTo` 4,481; rows built max **15**; textures **96** (cap reached and held; 1,186 decoded over the scroll). PSS **74.3 → 76.9, +2.6 MiB** (bar ≤ 10). **PASS** |
| S1 sabotage (Songs, no scroll) | loaded 100, 1 page, `shownTo` 15: L3 **FAIL**, as required |
| Did it run | radeonsi in every status; frames painted; L1 and L3 fetched more than one page |

The L3 delta across live-2 to live-5 was +2.4, +2.9 (live-4) and +2.6 MiB.

**Click paths (post-registration check, at fa91ced): PASS.**
- **Songs:** clicking song 4,481 played that song.
- **Albums:** clicking the last of 376 albums played one of that album's tracks.
- In both cases it was Playing with no banner, through the null sink only.

## What the earlier runs found (all fixed, with regression tests)

- **live-1:**
  - **A stale want fetched page 2 unasked.** While a list's first page was in flight, every frame re-armed the list's want, so the next page fired as soon as the first landed. S1 caught it: 200 rows with no scroll.
  - **`--scroll-end` didn't scroll.** It passed `f32::MAX` as the scroll offset, which gave an empty viewport. That flag is dev-only.
- **live-2:** L2 **FAIL**. `setQueue({playlist: p.…, startWith: 249})` started at track 1.
- **Diagnostics:**
  - `startWith` was also ignored for a queue of library song ids: song 4,481 was clicked and the window's first song played.
  - It was honoured only for playlist-track ids.
  - Fix (fa91ced): a click queues the clicked song plus up to 99 following loaded ids, with start 0. The ceiling is that "previous" can't reach earlier rows, and playback stops after 100 songs.
- **The harness itself:**
  - A stale status file from an earlier run in the same directory satisfied one diagnostic's waits. `launch` now deletes it first.
  - L2 now waits for a now-playing item before comparing.

## Memory (report only)

Dev build, single session, live-5:

| Reading | PSS MiB |
|---|---|
| Home control, never played | 73.7 |
| Songs route, never played (R2-library) | 74.3 |
| Songs scrolled to the end, 4,481 rows loaded | 76.9 |
| Playlist route open, playing (R1-library) | 171.5 |

Library textures sit in VRAM/GTT, not PSS. The bound on them is the LRU cap (96, held live) and the unit test, which checks that `tex_manager().num_allocated()` falls on eviction.

## Acceptance gate

| Run | Commit | R1 PSS / USS / RSS | R2 PSS / USS / RSS | Ballast (MiB, rise, excess) | Gate |
|---|---|---|---|---|---|
| s9-1 | aefb431 (clean) | 170.5 / 143.7 / 203.1 | 73.4 / 46.6 / 105.9 | 72, 75.3, **+3.3** | **FAIL** (control) |
| s9-2 | aefb431 (clean) | 171.0 / 144.1 / 203.7 | 75.2 / 48.4 / 108.0 | 71, 74.3, **+3.3** | **FAIL** (control) |
| **s9-3** | **e1f5adb (HEAD, clean)** | **171.0 / 142.7 / 205.2** | **73.5 / 45.1 / 107.6** | 73, 75.0, +2.0 | **PASS** |

- **Gate result.** R1 and R2 PASSed in zone 3 in every run. s9-1 and s9-2 failed only because the ballast's USS rise beat its size by 3.3 against a 3.0 tolerance.
- **The control is noisy on other branches too.** The ballast build also keeps the 50 cover RGBAs (about 1.8 MiB), so a small positive excess is expected. Same-day excess elsewhere: main (S7's `main-s7delta-2`) +0.8 at R1; S7's `s7-2` +2.5 at R1 and +3.3 at R2.
- **The stopping rule held.** It was set and committed before s9-3, and s9-3 held at +2.0.
- **The shipped guard passed.** `check-shipped-features.sh` exits 0 on the release binary in every run.

**Delta vs main.** Same day, other session: main f35def2 (code-identical to this branch's base) read R1 172.8, R2 74.7, with the reference at 88.7. Taking the lead over the reference:
- **R1:** main 84.1, s9-3 81.4 (−2.7).
- **R2:** main −14.0, s9-3 −16.1 (−2.1).

Different sessions, so a 2–3 MiB gap is noise, not a saving. S9 costs nothing measurable on the gate's Home scenario, which does load one page of sidebar playlists.

## Screenshots

- **Public:** `docs/screens/s9/songs-synthetic.png`. Synthetic fixture at 1100×720, with fake names and flat-colour art.
- **Private (real art and names, never published):** `target/screens/PRIVATE-real-art/s9/{songs,albums,playlist}.png` in the S9 worktree.

## Not established

- The R1/R2-library readings come from the dev build in a single session, without the reference binary; they are not R1/R2 readings.
- Library `l.` album ids in `setQueue` were proven for one album only.
- Catalog `pl.` playlists from search still show the placeholder; that is S8's.
