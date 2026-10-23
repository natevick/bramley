# S49 results: macOS library-playlist play error (2026-10-23)

Pre-registration: `docs/s49-PREREG.md` with Amendments 1 to 3. Counted run: Air clock 13:54 to 14:07, signed Developer ID dev-feature builds, muted, built-in speakers. Counts and verdicts only; no names or ids.

Binaries (signed sha256):
- control, v0.7.1 source 207757e: 080484be74170018823e17d2a13c3ebfd2b45b3f4c6f0c9284e7fa43688a5808
- fix, 0c5df50: 0958709098b5f9a29885c123f9f500c05da4b3f36a99461d42c11461661fda0c

Every UI case launched the app with `--route ... --click-row N`, which reaches `Library::play_from` through the render hook. FIX-err and its control used `bramley play 0` only. Playing means `queueLength > 0` with `queuePosition` set; the app's optimistic `Playing` with an empty queue (3 samples per playing case) is excluded.

## Verdicts against the pre-registered bars

| Case | Build | Result | Verdict |
|---|---|---|---|
| CONTROL: row 0, all-`a.` playlist (99 rows) | v0.7.1 source | 0 real-Playing samples in 24; banner at t=1 s; Code=1 and MPMusicPlayerControllerErrorDomain in 23 of 24 samples; `queueLength` 0 | reproduces, valid |
| FIX-a0: row 0, same playlist | fix | Playing at t=3 s, `queuePosition` 0 and stable, `queueLength` 99, position moving by t=5 s, no banner | PASS |
| FIX-aN: row 49, same playlist | fix | Playing at t=2 s, `queuePosition` 49 and stable, `queueLength` 99, moving by t=4 s, no banner | PASS |
| FIX-i: row 0, all-`i.` playlist (88 rows) | fix | Playing at t=2 s, `queuePosition` 0, moving by t=4 s, no banner | PASS on the bar; see finding 1 |
| FIX-songs: Songs-list row 0 | fix | Playing at t=2 s, `queueLength` 98, moving by t=4 s, no banner | PASS |
| FIX-err: `bramley play 0` | fix | banner is the underlying request error (MusicDataRequest 404); no Code=1, no MPMusicPlayerControllerErrorDomain | PASS |
| FIX-err control: same command | v0.7.1 source | banner has Code=1 and MPMusicPlayerControllerErrorDomain | control reproduces |
| control-i0 (report-only): row 0, all-`i.` playlist | v0.7.1 source | plays at `queuePosition` 0 by t=4 s moving; now-playing title and duration match API row 0 | report-only |

Clicked-row identity (Amendment 1):
- FIX-a0: title match yes, duration match yes.
- FIX-aN: title match yes, duration match yes.
- FIX-i: title match NO, duration match yes (not a bar for this case; see finding 1).

H1 verdict: CONFIRMED. CONTROL reproduces, and FIX-a0 and FIX-aN pass on the right song. Now-playing ids are numeric catalog ids in every case, so an id match against `a.` rows could never work; the start is by index.

VOID checks: other bramley processes during sampling 0 of every case; output muted and built-in speakers confirmed before each launch; control reproduced; volume restored exactly; no bramley process left. Pacing: 5 playing starts in 13 minutes, cases 75 s apart (the bar is at most 20 license actions per 10 minutes).

## Findings

1. **Container start is the wrong song on the `i.` playlist.** FIX-i played the track that is API row 59, not row 0, with the same `queuePosition` 0. The control, using the Songs path on the same playlist and row, played API row 0. So MusicKit's playlist track order differs from the API order for that playlist, and the index start lands on another song. On the `a.` playlist the order agrees (two indices checked, both match). Not fixed here. The container path now also covers `i.` playlists, where the Songs path worked; a candidate remedy is to use the container only when the clicked row is an `a.` id, but that is untested and mixed playlists were not run.
2. **The fix-err banner is a raw Swift error dump** (the request error with status, code and response details). It is correct per the bar and unhelpful for users. A follow-up, not part of this change.
3. **What the run did not exercise.** The `resolved 0 of N asked` message and the partial-resolve skip: the request threw, so the first error was rethrown. The gate (fix 2) is shown only by the banner not being Code=1.
4. **The first run (13:37 to 13:48) is void for fixes 2 and 3.** The FIX binary was a stale build (see Amendment 3). Its container results (fix 1) agreed with this run.

Evidence on the Air, `~/bramley-spike/s49/`: `run-cases-20261023-135451` (counted), `run-cases-20261023-133751` (void), `build2/`, `apps2/`, `gate/` (compile gate: clippy and `test --no-run` clean, macos-player 12 passed, library tests 57 passed on 0c5df50).
