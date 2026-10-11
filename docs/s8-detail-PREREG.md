# S8 album and playlist detail: pre-registration (2026-10-11)

Written before any live run of the S8 build. Thresholds do not move after data comes in.

## What runs

`scripts/app/s8-session.py`, run under `sg render`. It uses weston headless `--renderer=gl` on the real GPU, a PipeWire null sink passed both as `PULSE_SINK` and `--output-device`, and the sink guard from `covers-session.py`. Every run uses an isolated copy of the config dir and a throwaway keyring service. The binary is the `dev` build of the commit under test.

Dev-only flags stand in for a user's clicks: `--click-cover <n>` (Home cover n, from 0), `--click-track <n>` (track n of the open detail view, from 1) and `--click-first-playlist` (the first catalog playlist in the search results). Seeking goes through the shipped CLI, `bramley seek <s>`, against the running instance.

The status file never holds titles or artist names (the `title`/`artist` keys are dropped before writing). Verdict files hold booleans, indices and counts. For the leak grep, the harness writes the names it sees through the shipped CLI (`bramley now`, `bramley search`) only to the gitignored `target/tmp/s8/names.txt`.

## Checks and pass conditions

**L1. Home cover, play track 1**
- `--click-cover 0 --click-track 1`.
- Within 30 s: `detail.state == "loaded"`, `detail.art` true, `state == "Playing"`, `itemId == detail.trackIds[0]`, and the position advances by at least 2 s between two samples.
- The guard saw only the null sink and never fired.

**L2. F6 album playthrough (acceptance.md F6), plus the plan's AC**
- The album is the first Home cover, in chart order, whose detail has at least 6 tracks. Start at track N = 3: `--click-cover k --click-track 3`.
- **Plan AC:** `itemId == trackIds[2]` and Playing within 10 s of the detail loading.
- **Three transitions.** For each of tracks 3, 4 and 5: once it is the item and its duration is known, record `drainsAcked`/`drainTimeouts` and run `bramley seek <duration − 20>`. Then wait up to 40 s for `itemId` to equal the next track's id.
- **A transition PASSes when:**
  - at the first status showing the next track's id, `drainsAcked` is at least one more than before the seek and `drainTimeouts` is unchanged;
  - the last position seen on the old track was at least its duration − 3 s, so no `next` cut it short.
- **F6 PASSes** only if all three transitions pass, with the guard silent and null sink only.

**L3. Catalog playlist from search, play from track 5**
- `--route search --search "bohemian rhapsody" --click-first-playlist --click-track 5`.
- Within 30 s: the route is Playlist, the detail has at least 5 tracks, Playing, `itemId == trackIds[4]`, and the position is advancing.
- The guard saw only the null sink.

**`startWith` for catalog containers (finding, no bar)**
- The first build maps a click to `setQueue({album|playlist: id, startWith: i})`.
- If L1, L2's AC or L3 shows an `itemId` other than the clicked track's while Playing, `startWith` is not honoured for that container type. The mapping then switches to S9's approach (the clicked song plus up to 99 following, start 0) and the checks re-run. Both results are reported.

**Did it run**
- radeonsi appears in every status, and frames are painted.
- A missing status file is a FAIL.

**Sabotage F6-S**
- The same L2 procedure, except that for the third transition the harness sends `bramley next` 3 s before the end instead of letting the track end.
- That transition must FAIL: either no new drain acknowledgement, or a last position short of duration − 3 s. If F6-S passes, the F6 check cannot see a cut tail and counts for nothing.

## Gate

- `sg render -c 'bash scripts/acceptance.sh s8-1'` must PASS as acceptance.md defines, after its own pre-flight.
- The delta vs main is taken on the app's lead over the same-session reference binary, against main's last recorded gate (S7's s7-4).
- **Stopping rule:** s8-1 runs once at the final HEAD on a clean tree. If it fails only on the ballast control, s8-2 runs once and the work stops there; any further failure goes back as a blocker.

## Screenshots

- **real:** album detail opened from Home, and the playlist detail from L3, at 1100×720. Written only to `target/screens/PRIVATE-real-art/s8/` and never committed.
- **synthetic:** a loopback fixture (flat-colour art, fixture names and tokens) serves the chart, one album with tracks and the art. One shot goes to `docs/screens/s8/`. The script refuses to write either set into the other's directory.

## Named false positives

- An `itemId` equal to the clicked id can also mean a one-song queue. L2's auto-advance is what proves the rest of the container was queued.
- A drain acknowledgement can come from a sample-rate change mid-stream, not only from the end of a track. The bar therefore also requires the old track's last position to reach duration − 3 s.
- `drainsAcked` only grows. If it is read from one media element and MusicKit swaps elements, acknowledgements are lost. The engine sums all live elements plus those already destroyed, and a unit test covers the destroyed case.

## Amendment 1 (2026-10-11, after live-1 and before any further counted run)

**What live-1 found.**
- L1 passed. L2's AC passed: track 3 played 1.05 s after the detail loaded. L3 never clicked, because "bohemian rhapsody" returns no catalog playlists.
- Every F6 transition froze about 1.5 s before the track's end. Diagnosis traced it to an engine bug: a last AAC frame shorter than 2 ms was decoded forever. That is fixed in 1ea15e9, with a regression test.
- With the fix in, the album played through the boundary into the next track. It did so **gaplessly**:
  - MusicKit appends the next track to the same SourceBuffer and never calls `endOfStream`;
  - the native position runs on past the old track's end;
  - no drain happens at the boundary (`drainsAcked` stayed 0).
- acceptance.md's F6 ("each track's drain is acknowledged before the next starts") therefore cannot pass as written. It assumed an end of stream per track. Changing F6 is Nate's call, so it is not changed here.

**F6-literal (unchanged, reported).** Per transition: `drainsAcked` up by at least one and `drainTimeouts` unchanged, at the first status showing the next id. Expected to FAIL under gapless playback. It is reported with the evidence.

**F6-gapless (post-registration, labelled as such).** It checks the intent behind F6, no cut tail, under gapless playback. Positions: `position` is native timeline time, while `duration` and `bramley seek` are item-relative. Each item's timeline start is the native position first seen on that item; the first track, started by `setQueue`, starts at 0. A transition PASSes when:
- the seek landed within 1 s of `start + duration − 20`, so the last 20 s were actually played;
- the old item's last position, item-relative, reached `duration − 3`;
- across the id flip, the native position never moved backwards, and never jumped forward by more than the wall time between the two samples + 0.3 s;
- the guard saw the null sink only.

F6-S must FAIL F6-gapless.

**Blind spot.** A flush that drops only the sink's ~200 ms server buffer at the boundary sits near the 0.3 s allowance and the 250 ms sampling. F6-gapless cannot see it.

**L3 term.** `"queen essentials"` replaces "bohemian rhapsody". The shipped `bramley search` returned 10 catalog playlists for it before this amendment. If the results hold no playlist, the harness reports that separately from a failed click.

## Amendment 2 (2026-10-11, after live-2 and before live-3)

**What live-2 found.**
- L1, L3 and L2's AC passed.
- MusicKit uses **two** boundary modes, and picks between them per transition:
  - **Mode A, gapless.** The next track is appended to the same SourceBuffer, the position continues, and no drain happens. This was live-2's L2 transitions 2 and 3.
  - **Mode B, ended.** The old track ends, the drain is acknowledged (`acked` 1, `timeouts` 0), and the next track starts on a new timeline at 0. This was live-2's L2 transition 1.
- Amendment 1's F6-gapless failed mode B on its "never moved backwards" rule, even though mode B is the drain-acknowledged end that F6 asks for.

**F6-gapless, revised (post-registration, labelled as such).** A transition PASSes when:
- the seek landed within 1 s of `start + duration − 20`;
- the old item's last position, item-relative, reached `duration − 3`;
- **either** mode A's continuity holds (no backward move, and no forward jump bigger than the wall time + 0.3 s), **or** mode B's F6-literal condition holds (`acked` ≥ 1 and `timeouts` = 0).

F6-literal is unchanged and still reported. F6-S must still FAIL. In live-2, F6-S's third transition was 3.96 s short of its end.

## Amendment 3 (2026-10-11, after live-3, procedure only)

In live-3, F6-S's first seek went out about 1 s after playback started. It was acknowledged, but the position never moved, so that run failed for a reason unrelated to the sabotage. From now on the harness seeks only once the item's position is more than 2 s past its timeline start. No bar changes.

## Amendment 4 (2026-10-11, after the S8 review and Nate's F6 ruling, before live-6)

**Withdrawn.** The F6-gapless PASS from live-3 and live-5 is withdrawn.
- Its sabotage sent `next` at `duration − 4 s`, not the registered 3 s.
- At 3 s the cut would have fallen inside the 3 s tail bar.
- The continuity rule compared only the pair of samples at the flip. That pair can straddle a reset, so it missed the cut.

**F6 (acceptance.md as amended 2026-10-11, approved by Nate).** A transition PASSes when the seek landed within 1 s of `start + duration − 20`, and **either**:
- **(a)** the drain was acknowledged: `drainsAcked` up by at least 1 and `drainTimeouts` unchanged, measured from the seek being applied to the id flip; **or**
- **(b)** continuous gapless output, all three of:
  - media time sent to the output from the applied seek to the flip ≥ `(duration − target) − 3 s` (engine `sentMs`);
  - media time skipped ≤ 0.1 s (engine `skippedMs`: gaps in the sent timeline plus queued audio discarded by a flush or detach);
  - no position stall longer than 1 s in the 5 s window either side of the flip.

**F6-S** sends `next` at the registered 3 s before the end, on transition 3, and must FAIL.

**L5 (review #5).** A catalog playlist longer than 100 tracks, row 105 clicked: `pl.f68f…`, 111 tracks, the longest of 12 searched. Row 150 was asked for, but no catalog playlist found had that many. Live probe at 1921d0e's mapping (`startWith` 104): **row 1 played**. The mapping now queues a window: the clicked song plus up to 99 playable songs after it. L5 PASSes when row 105 plays with the position advancing and the null sink only.

**Report only (review #6).** Two click sequences, 12 s apart: "Shuffle, then track 3", and "track 5, then Shuffle". The rows played are recorded.

**Names (review #2).** The names file is created 0600 and removed in a `finally`. The leak grep runs inside the session and records only counts, plus whether a planted name raises the count.

## Amendment 5 (2026-10-11, after live-6, before live-7)

**What live-6 showed.**
- F6-S **PASSed** amendment 4's F6, so F6 counts for nothing in live-6.
- The `next` cut on transition 3 (2.89 s short) registered as `skippedS` 0.0, because amendment 4 measured sent and skipped only up to the id flip.
- The cut's discard is counted, but after the flip: F6-S's run total was 22.8 s skipped, against L2's 4.1 s from its three seek discards alone.

**Change.** Sent and skipped are now measured from the applied seek to the end of the 5 s window after the flip. That is the same window as the stall check. The value at the flip is kept as `skippedAtFlipS` for evidence. No threshold changes. F6-S must FAIL in live-7.
