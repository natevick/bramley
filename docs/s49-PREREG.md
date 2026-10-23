# S49: macOS library-playlist play error, pre-registration (2026-10-23)

Symptom: on macOS, playing a library playlist (`p.`) shows `Playback error: NSError ... MPMusicPlayerControllerErrorDomain Code=1`. Songs-list rows and Albums-list albums play.

Leading cause (H1): library-playlist entries carry `a.` ids. `wire.rs` routes them as catalog ids, nothing resolves, SetQueue fails ("no playable items"), and the Play that follows fails with Code=1 on the empty queue and masks it. Evidence so far is cache-level only, counts by id kind: of 5 cached library playlists, 4 are 100% `a.` and 1 is 100% `i.`; the Songs list is 100% `i.`.

Change under test (base 207757e):
1. On macOS, a row in a `p.` library playlist plays the playlist as a container at the clicked index (`SetQueue` kind `playlist`, `library: true`, `start` = row index). Linux behaviour is unchanged: musickit.js ignored `startWith` for `setQueue({playlist: p.…})` (S9 live-2), so Linux keeps queueing the clicked song and the ones after it.
2. After a failed SetQueue, the Play that follows is not sent, so the banner shows the SetQueue error.
3. Swift song resolution skips a batch that throws or misses instead of failing the whole SetQueue. An empty queue reports resolved/asked counts, and if every batch threw, the first error.

## Phase B: Air, muted playback

Setup:
- Signed build via the GUI-session `.command` flow used in earlier releases (ssh cannot unlock the login keychain). The Air is unlocked and idle.
- Mute the built-in output before any playback, require the default output to be the built-in speakers, capture the volume settings first, restore them exactly afterwards with a trap.
- Each case starts from a fresh launch.
- Every command reply is logged with its seq. No playlist, song or album names or ids are written anywhere committed or reported; counts and id kinds (`a.`/`i.`) only.
- Apple pacing: at most 20 license actions per 10 minutes.

Cases and bars, fixed now:

| Case | Build | Pass |
|---|---|---|
| CONTROL: row 0 of an all-`a.` library playlist | v0.7.1 `/Applications` build | reproduces the failure: SetQueue reply "no playable items" and Play Code=1, or Code=1 alone. If it plays, the run is VOID. |
| FIX-a0: row 0 of the same all-`a.` playlist | fix build | reaches Playing at queue index 0, position moving within 10 s |
| FIX-aN: a mid row (index N, 0 < N < count) of the same playlist | fix build | reaches Playing at queue index N, position moving within 10 s |
| FIX-i: row 0 of the all-`i.` playlist | fix build | reaches Playing at index 0, position moving within 10 s |
| FIX-songs: one Songs-list row | fix build | reaches Playing, position moving within 10 s |
| FIX-err: a forced SetQueue failure | fix build | the banner shows the SetQueue message ("no playable items ..." with resolved/asked counts or the underlying error), not Code=1 |

"Position moving" means two samples at least 2 s apart where the second is greater, state `Playing`, and the queue position equal to the clicked index.

VOID conditions: the output is not muted or the speakers are not the built-in device; a SetQueue reply is missing from the log; the CONTROL does not reproduce; the build under test is not the one running (pid by exact binary path); another bramley process is running.

H1 verdict: CONFIRMED only if CONTROL reproduces AND FIX-a0 and FIX-aN pass. If CONTROL reproduces but FIX-a0 fails, the container request is not the whole story and the result is reported as a failed fix, not tuned.

Not covered by this run (stated now): Linux playback of this change (the macOS gate leaves it unchanged by construction); playlists mixing `a.` and `i.`; playlists containing music videos.

## Amendment 1 (2026-10-23, before any Mac run)

Written after reading `playback.rs`, `cli.rs`, and the Swift snapshot code, still with no Mac data. It supersedes the table and VOID rules above where they disagree.

- **Reply log dropped.** v0.7.1 does not log command replies, so "a SetQueue reply is missing" would void the CONTROL by construction. Cases are judged on `bramley now` fields only: `state`, `position`, `queuePosition`, `itemId`, `banner`. The CONTROL must show `banner` set and `state` not `Playing` within 10 s of the click.
- **Index bar scoped to the container cases.** `queuePosition` is absolute in the MusicKit queue entries (Swift `entries.firstIndex`), so "equal to the clicked index" applies to FIX-a0, FIX-aN and FIX-i only. FIX-songs queues from index 0 by design; its bar is `Playing`, position moving, no banner.
- **Index is not the song.** FIX-aN and FIX-a0 also need the now-playing track to be the clicked row: compare `title` and `duration` from `bramley now` against the cached API row at that index, in scratchpad only. Report match yes/no. A "no" fails the case even if the index matched, because it means MusicKit's track order and the API rows disagree and the index start is wrong.
- **FIX-err mechanism.** `bramley play 0` (the CLI builds `Queue::Songs(["0"])`, a catalog id nothing resolves). Pass: `banner` carries the SetQueue message ("no playable items for the queue", with `resolved 0 of 1` or the underlying request error) and does not contain `MPMusicPlayerControllerErrorDomain` or `Code=1`. The same command on v0.7.1 is run once as a second control and is expected to show Code=1; if it does not, FIX-err is reported as inconclusive, not passed.
- **Mac compile gate before bundling.** On the Air, `cargo clippy -p bramley --all-targets -- -D warnings` and `cargo test -p bramley --no-run` must pass, and the Swift module must build. The Swift and `player.rs` edits cannot compile on Linux, so a bundling failure here is a failed gate, not a Phase B result.
- **Linux gate evidence.** The container path is macOS-only (`cfg!(target_os = "macos")`). Evidence for keeping Linux on `Queue::Songs`: S9 live-2 (`docs/s9-library-RESULTS.md`), where musickit.js `setQueue({playlist: p.…, startWith: 249})` started at track 1.
- **Not changed, stated now.** `a.` ids in a `Queue::Songs` queue still route to the catalog batch: nothing shows what MusicKit returns for them in either request, and the container path no longer sends them. Fix 4 (carrying `catalogId`) is not part of this change.

## Amendment 2 (2026-10-23, before any case ran)

- **CONTROL is not the /Applications binary.** The shipped v0.7.1 has no `--click-row`, and driving its UI needs Accessibility grants. CONTROL = a `--features dev` release build of the v0.7.1 source (207757e), Developer ID signed, run from `~/bramley-spike/s49/apps/ctl`. The FIX build is the same flags on 32d5d39's code (0c5df50 differs only in a test file), run from `apps/fix`. /Applications is not touched. Everything else about the control (the bar, the VOID rules) is unchanged.
- **Driver.** Every UI case launches the app with `--route <playlist:<id>|songs> --click-row N`, which reaches `Library::play_from` through the render hook. FIX-err and its control use `bramley play 0` only.
- **Signed sha256 of the binaries under test.**
  - control (207757e, dev): 2643b9c244659b292ffec6cd191d9280fa91e591bda6e53a6421b4ad2f205fc4
  - fix (32d5d39 code, dev): f74347076d7e59e36c24dc09259c2342502ce49a98856f770de4f19579fbf3e2
- **Timing.** "Position moving within 10 s" is measured from the first sample where the list has loaded (the click happens on the next frame), and also reported from first Playing. Both are recorded; the first is the bar.

## Amendment 3 (2026-10-23 13:55, before the re-run; after the first run's data)

The first run (13:37 to 13:48) is VOID for fix 2 and fix 3 and kept as report-only for fix 1.
- **Cause: the FIX binary was a stale build.** Both trees were built into one cargo target dir. `git archive` stamps every file with the commit time, which was older than the control build's outputs, so cargo reused the control's `bramley-macos-player` (Rust `QueueGate` and the Swift library). The FIX app had the container change (library.rs) but not the gate or the Swift skip. Evidence: no `resolved 0 of` string in that binary, and FIX-err showed Code=1 like its control.
- **Fix.** Each tree is rebuilt in its own target dir (`target-s49-ctl`, `target-s49-fix`) after touching every source file, with a content check: the FIX binary contains `resolved 0 of`, the control does not. The FIX tree is 0c5df50.
- **Binaries under test (replace Amendment 2's).** Signed sha256, `apps2/`:
  - control (207757e, dev): 080484be74170018823e17d2a13c3ebfd2b45b3f4c6f0c9284e7fa43688a5808
  - fix (0c5df50, dev): 0958709098b5f9a29885c123f9f500c05da4b3f36a99461d42c11461661fda0c
- **"Playing" is redefined.** The app shows an optimistic `Playing` with an empty queue for the first 0 to 2 s after a click. A sample counts as Playing only with `queueLength > 0` and `queuePosition` set. Optimistic samples are counted and reported. This changes no bar; it removes a false positive, and the first run's analysis had it.
- **Added report-only diagnostic: control-i0.** The control build, row 0 of the all-`i.` playlist. In the first run FIX-i reached index 0 and moved, but its now-playing title did not match the API row (duration did). This case shows whether the old Songs path matches that row. It does not affect any bar.
- All other cases, bars and VOID rules stand. The whole case set is re-run on the new binaries; the re-run is the counted run.
