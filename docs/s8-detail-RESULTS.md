# S8 album and playlist detail: results (2026-10-11)

Pre-registration: `docs/s8-detail-PREREG.md`, with amendments 1–3. Live harness: `scripts/app/s8-session.py`, run under `sg render` with weston `--renderer=gl` (radeonsi), a PipeWire null sink passed as both `PULSE_SINK` and `--output-device`, the sink guard, and an isolated config and keyring. The default sink was unchanged after every session, and every null sink was removed.

Commits were rebased onto main after S9 merged. The old hashes in amendment 1 map like this: 1ea15e9 → 6b7b59d (engine hang fix).

## Review round (2026-10-11): current results

**Rebased onto main 750bfc2**, which carries the engine short-frame fix and its tests. The fix's S8 copy is dropped, and `crates/` is identical to the gated build eef8d07, now b91735a. Gate and live hashes below are the pre-rebase builds that actually ran. This section supersedes the round-1 results below wherever they differ. Inputs were the S8 review and Nate's F6 ruling, now recorded in acceptance.md: "no cut tail: drain acknowledged (end of stream) or continuous gapless output". PREREG amendments 4 and 5 hold the measurable form.

**Withdrawn: the round-1 F6-gapless PASS** (live-3, live-5).
- Its F6-S sent `next` at `duration − 4 s`, not the registered 3 s, and that deviation was not disclosed.
- Its continuity rule compared only the two samples at the flip.

### Live, final: live-8 plus an F6-S re-run (dev build of eef8d07), PASS

| Check | Result |
|---|---|
| L1 Home cover → track 1 | Track 1 plays, art shown, null sink only. **PASS** |
| L2 plan AC: start at track 3 | Track 3 in 1.07 s. **PASS** |
| L2 F6, 3 transitions | T1 mode B: drain acked 1, 0 timeouts. T2 and T3 mode A: skipped 0.0 s, sent ≥ 24.5 s (bar 17 s), stall ≤ 0.28 s. Seeks landed within 0.41 s. **PASS** |
| F6-S, `next` 3 s before the end (registered) | Re-run, see below. T1 and T2 pass. T3: **skipped 1.5 s** (bar 0.1), tail 2.85 s short. F6 **FAILs**, as required |
| L3 catalog playlist from search → track 5 | **PASS** |
| L5 row 105 of a 111-track catalog playlist | Row 105 plays, through the window fallback. **PASS** |

**Why F6-S was re-run.** In live-8 the sink guard killed the app during F6-S's transition 2 (`pw-dump` timed out), before the sabotage could run. That is the guard failing safe, and not a valid F6-S. F6-S was re-run on its own at the same build.

**Earlier runs in this round.**
- **live-6** (cf7838e): F6-S **passed** amendment 4, so F6 counted for nothing in that run. Skipped media was read at the id flip, and the cut's discard is counted just after it. Amendment 5 moved the reading to flip + 5 s, and acceptance.md says the same.
- **live-7** (cf7838e): all PASS, and F6-S FAILed with skipped 18.7 s. That magnitude was a bug: `detach` paused before `discard_output`, so the "queued" span ran from the stale seek anchor. Fixed in 6168ee1; the test read 4.02 s before the fix. With the fix, F6-S reads 1.5 s, about one decode-ahead.
- **The definition's weak point:** only the skipped count catches a cut. A 3 s cut still read 17.8 s sent at the flip, against a 17 s bar. acceptance.md says this.

### `startWith` (review #4, #5)

- MusicKit drops songs without `playParams` before it applies `startWith`. Queue positions now count only what it keeps: playable songs, plus music videos. Unplayable rows are greyed out and can't be clicked.
- I could not cite a documented string-id `startWith`, so it is not used.
- **Container queues hold only page 1.** At round 1's mapping, clicking row 105 of a 111-track playlist played row 1. At HEAD, clicking row 100 (the last row of page 1, through the container) plays row 100, so the switch at Bramley's first page matches MusicKit's. Rows past MusicKit's first page now queue the clicked song plus up to 99 playable songs after it (S9's window).
- Row 150 was asked for, but the longest of the 12 catalog playlists searched had 111 tracks.

### Shuffle (review #6)

Shuffle(false) is now sent before SetQueue. Live, clicks 12 s apart:
- **Shuffle, then track 3:** played rows 1, then **3**. The unshuffled queue starts where it was clicked.
- **Track 5, then Shuffle:** played rows 5, then **1**. Shuffle always starts at container place 0, and MusicKit keeps that item first.

### Engine

**Branch `natevick/engine-short-frame`** (aafea34, db10d22) lands ahead of S8:
- the hang fix;
- review #8: exactly one write at the last pts, with round(272 µs × rate) frames;
- review #9: a two-append gapless boundary starting 1 µs inside the short frame, with every pts written once;
- review #10: both tests run under `in_child` with `timeout 20`.

Mutants E1–E4 are each killed, and a no-change control passes. My first mutant run used `cargo test -q`, whose output hides the per-test lines, so every mutant read SURVIVED. That was a false clean. The runner now also checks the return code and the test count.

**On S8** (05dbac7): `sentMs` and `skippedMs` reach the status. `skippedMs` counts sent-timeline gaps over 2 µs, plus queued audio discarded by a seek, flush or detach. Four counter mutants are killed.

**Found, not fixed:** with a frame missing mid-stream, position (driven by samples) never reaches the sent end, so `ended` never fires. A decrypt failure drops a frame the same way. This needs its own story.

### Tests this round

- Mutants killed:
  - **A1–A3:** an accesskit click test on the real row, Play and Shuffle widgets. Its disabled-state assertion also kills N5.
  - **N1–N7:** unplayable counted, no window fallback, the cap reading as complete, `l.` routed, an unplayable row clickable, unshuffle after SetQueue, `playParams` ignored.
- The no-change control passes.

### Gate s8-4 (eef8d07, clean): PASS in zone 3

- R1: **172.3** PSS / 145.5 USS / 204.9 RSS.
- R2: **75.4** / 48.5 / 108.0.
- Reference: 89.7 / 89.6.
- Ballast control holds: 70 MiB, rise 71.0.
- The shipped guard exits 0, and the ballast build's exits 1.
- On the lead over the reference, against s9-4: R1 **−1.6**, R2 **0.0** MiB. That is noise.
- s8-3 (e956bff) also PASSed, before the detach fix.

### Names (review #2)

- The names file is opened with `os.open(..., 0o600)`, grepped inside the session, and removed in a `finally`.
- live-7: 63 names, 2 hits, "Queen" and "Queen Essentials". Both are public catalog text that come from the harness's own search term.
- A planted name raises the count.

### Budget this round (about 150 / 150)

- **S8 source:** +105 / −43.
- **S8 tests:** +136 / −31.
- **Harness:** +80 / −33.
- **Engine branch tests:** +61.

### Not established (this round)

- **#11:** music videos are queued (MusicKit keeps them) but not listed. Auto-advance can reach a video the list doesn't show, and v1 fails it loudly.
- Shuffle order beyond the 2 rows observed per sequence.
- Rows past 111 in a real playlist. The window path past page 1 was proven live only at row 105, and L5 would also pass on a one-song queue. That the window queued the songs after row 105 is unit-tested only, the same caveat S9 recorded.

## Round 1 live checks: live-5, dev build of 5d6ae45 (F6-gapless PASS withdrawn, see above)

| Check | Result |
|---|---|
| L1 Home cover → track 1 | Album route; 15 tracks; header art; the clicked track playing; +2.25 s advance; null sink only. **PASS** |
| L2 plan AC: start at track 3 | Track 3 was the item, Playing, **0.79 s** after the detail loaded (bar 10 s). **PASS** |
| L2 F6-gapless, 3 transitions | Seek landed +0.24 / −0.21 / +0.19 s from `duration − 20`. The tail fell short by −0.25 / 0.57 / 0.50 s (bar ≤ 3). Continuous; all gapless. **PASS** |
| L2 F6-literal | 0 drain acks on gapless transitions. **FAIL as written**, see below |
| L3 catalog playlist from search → track 5 | Playlist route; 25 tracks; art; track 5 playing; +2.21 s. **PASS** |
| F6-S sabotage (`next` 3 s before the end, transition 3) | Transitions 1 and 2 pass. Transition 3 fell **3.83 s** short, so F6-gapless **FAILs**, as required |

**`startWith` for catalog containers: honoured.** `setQueue({album: id, startWith: i})` started at track 3 (L2 AC, every live run). `setQueue({playlist: pl.…, startWith: 4})` started at track 5 (L3, live-2 to live-5). The view therefore queues the container itself, not S9's 100-song window. The start index is the song's place in the container, so a music video earlier in it cannot shift the start; videos are not listed.

**Shuffle** uses MusicKit's public `shuffleMode` (`songs`), set after `setQueue` and before `play`. Live probe: in two runs, the first song was track 1 both times, then 14, 8, 9 and 15, 2, 12. MusicKit shuffles the rest of the queue but keeps the start item first. A later Play or row click turns shuffle off again (unit-tested).

## F6: what round 1 found (Nate has since ruled; see the review round)

1. **Engine hang, fixed (6b7b59d).** A track whose last AAC frame is shorter than 2 ms (this album's track ended in a 272 µs frame) made `decode_ahead` decode that frame forever. The player thread froze about 1.5 s before the end, on every run. The regression test fails by timeout without the fix. Any track ending like this hit it, so S13's R3 soak would have too.
2. **MusicKit has two boundary modes.**
   - **A, gapless.** The next track is appended to the same SourceBuffer, with no end of stream and no drain.
   - **B, ended.** The drain is acknowledged (`acked` 1, `timeouts` 0), then a new timeline starts.
   - live-2 to live-5 saw both. live-5's L2 was all mode A.
3. **acceptance.md F6** requires a drain acknowledgement before each next track. Mode A never produces one, so F6 **cannot pass as written** for gapless transitions. The post-registration F6-gapless check covers the intent (no cut tail) for both modes; F6-S proves it can fail. **Decision for Nate:** whether F6 should read "no cut tail: either a drain acknowledged (end of stream) or continuous gapless output" before S13 wires it.
4. **Blind spot:** a flush that drops only the sink's ~200 ms server buffer at a gapless boundary is within the 0.3 s continuity allowance.

Drain counts now reach the status file from the shipped engine (`drainsAcked`, `drainTimeouts`), summed across live and destroyed media elements, so S13 can use them without the diagnostics build.

## Runs (all reported)

| Run | Build | Outcome |
|---|---|---|
| live-1 | pre-fix | L1 PASS, AC PASS. L3 never clicked ("bohemian rhapsody" has no catalog playlists). Every F6 transition froze: the engine hang |
| live-2 | engine fix | L1, L3, AC PASS. F6-gapless (amendment 1) failed mode B's backward timeline jump on transition 1 → amendment 2 |
| live-3 | e0a6eb1 | L1, L2, L3 PASS. F6-S failed on transition 1 instead: a seek 1 s into playback was acknowledged but not applied → amendment 3 (procedure) |
| live-4 | 5d6ae45 | Harness-invalid: the landing was read from a stale pre-seek status (−180 s); fixed |
| live-5 | 5d6ae45 | All PASS, as above |

**Finding outside S8:** a seek sent about 1 s after `play` is acknowledged (`applied` = `seq`) but MusicKit does not move. That is F5 territory, and S8 did not chase it.

## Tests

8 new app tests and 2 new engine tests, plus extensions to 2 engine tests. Each was killed by a recorded mutation, fail-before/pass-after. M3–M12 were re-run at HEAD after the rebase (a5966c8), and all 11 were killed:

| Mutation | Test that failed |
|---|---|
| M1 retired drain counts not kept | `drain_acks_count_across_tracks_and_outlive_the_element_that_played_them` |
| M2 frame lookup without the `end > cursor` filter | `a_last_frame_shorter_than_the_lookup_slack_…` (timeout) |
| M3 click ignores the song's container place | `a_click_plays_the_container_from_that_songs_place_in_it` |
| M4 `p.` library ids routed to detail | same |
| M5 / M5b detail and app ignore sign-in | `signed_out_nothing_is_fetched_…`, `an_open_album_reaches_the_api_worker_only_when_signed_in` |
| M6 stale replies accepted | `opening_fetches_once_pages_the_rest_and_drops_replies_for_another_album` |
| M7 videos listed / M8 partial total shown | `the_model_keeps_songs_with_their_container_place_…` |
| M9 Home cover not clickable | `clicking_a_home_cover_opens_that_albums_detail` |
| M10 shuffle flag dropped | `shuffle_sets_the_container_then_shuffles_and_the_next_play_unshuffles` |
| M11 next page refetched each tick | `opening_fetches_once_…` |
| M12 no loading deadline | `a_silent_or_failed_fetch_ends_in_an_error_not_a_spinner` |

## Gate

- `s8-1` (1921d0e): **VOID, did not run.** The reference binary is gitignored and lived only in the main checkout, so R1/R2 failed on "missing evidence: reference.json". It was copied in, and its sha256 prefix matches the recorded `1a72eb4b2880ac5a`.
- **`s8-2` (1921d0e, clean): PASS in zone 3.**
  - R1: **171.6** PSS / 144.7 USS / 204.2 RSS.
  - R2: **74.3** / 47.4 / 106.9.
  - Reference: 88.4 / 88.7.
  - Ballast control holds: 71 MiB, rise 73.2.
  - The shipped guard exits 0, and the ballast build's exits 1.
- `s8-2` is the single counted run with a working reference. The stopping rule had not anticipated a missing reference.
- **Delta vs main** (S9's s9-4, fd854b8, whose `crates` and `scripts` are identical to main: R1 174.0, R2 75.5, reference 89.7 / 89.8), taken on the lead over the reference: R1 **−1.2**, R2 **0.0** MiB. That is session noise. The gate's Home scenario adds only 50 album-id strings.

## Screenshots

- **Public:** `docs/screens/s8/album-detail-dark-1100x720.png`, from the loopback fixture (flat-colour art, fixture names).
- **Private (real art and names, never published):** `target/screens/PRIVATE-real-art/s8/20261011-122831/{album,playlist}-detail-dark-1100x720.png` in the S8 worktree.

## Names and leaks

- Status files never hold title or artist. Verdict files hold ids, indices and counts.
- The 263 names seen through the shipped CLI (search results and played tracks) went to the gitignored `target/tmp/s8/names.txt`, which was deleted after the grep. Library playlist names (in the sidebar since the rebase) and chart album titles were not in the list, so the grep could not have caught them. They were grepped against the branch diff, the commit messages, the docs and every live app log. The only hits are the two search terms the harness uses: "bohemian rhapsody", a public search term already used by S7, and "queen essentials", which is also the name of Apple's editorial playlist.
- Planting one listed name raised the count from 12 to 13, so the grep can fail.
- The token grep found nothing.

## Budget (plan cap 300 source / 150 tests / 40 fixtures)

**Over on source and tests.**
- **Source:** +437 / −32. `ui/detail.rs` is 278 of it; engine F6 support (hang fix, drain counts, shuffle) is about 33; the rest is wiring (app, covers, search `Art` reuse, playback, dev hooks).
- **Tests:** +282 / −1: `ui/detail/tests.rs` 141, app 83, engine 58.
- **Fixtures:** 0; generated in code and in the harness.
- **Harness:** `scripts/app/s8-session.py`, 224 lines, against S13's harness cap.
- **Why over:** the engine hang and the drain telemetry F6 needed, Shuffle, paging, and the stale-reply, deadline and sign-in guards with their tests.

## Not established

- 1921d0e (rows aligned left) landed after live-5. Only the screenshots and gate s8-2 cover it.
- F6 as written in acceptance.md (Nate's call, above).
- Shuffle's queue order beyond the 4 songs observed per run. No sabotage control for it.
- Catalog playlists over 100 tracks: the paging path is unit-tested only. The L3 playlist had 25.
- `bramley search` reaching a catalog playlist id is shown only through the dev click hook. There is no CLI play for containers.
