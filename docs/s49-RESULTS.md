# S49 results: macOS library-playlist play error (2026-10-23)

Pre-registration: `docs/s49-PREREG.md` with Amendments 1 to 4. Counted run: Air clock 14:21 to 14:37, signed Developer ID dev-feature builds, muted, built-in speakers. Counts and verdicts only; no names or ids.

Binaries (signed sha256):
- control, v0.7.1 source 207757e: 080484be74170018823e17d2a13c3ebfd2b45b3f4c6f0c9284e7fa43688a5808
- fix, 8db1932: 95f6aadf055b9a22643da81b04410d08a48e2f2bc68b450805c68423192e0c7b

Every UI case launched the app with `--route ... --click-row N`, which reaches `Library::play_from` through the render hook. FIX-err and its control used `bramley play 0` only. Playing means `queueLength > 0` with `queuePosition` set; the app's optimistic `Playing` with an empty queue (1 to 3 samples per case) is excluded.

## History

- Run 1 (13:37) used a stale FIX binary (cargo reused the control's `bramley-macos-player`), so it is void for the gate and the Swift skip.
- Run 2 (13:54) tested a container design: it played an `a.` playlist at the clicked row, but on an `i.` playlist it started on another song than the clicked row (API row 59, not row 0), a regression against v0.7.1. The FIX-i bar checked index and motion, not the clicked row, so it was too weak. That build (0c5df50) was a DEFECT and is not shippable. The container approach was dropped.
- Run 3 (this one) tests 8db1932 under the tightened bars of Amendment 4: rows are queued in list order, and `a.` rows are queued by their `playParams.catalogId` on macOS.

## Verdicts (run 3)

Title and duration are compared with the clicked API row at the first real-Playing sample.

| Case | Build | Result | Verdict |
|---|---|---|---|
| CONTROL: row 0, all-`a.` playlist (99 rows) | v0.7.1 source | 0 real-Playing samples in 25; banner at t=1 s, present in 24 samples, with Code=1 and MPMusicPlayerControllerErrorDomain; `queueLength` 0 | reproduces, valid |
| control-i0: row 0, all-`i.` playlist (88 rows) | v0.7.1 source | plays at t=2 s, moving by t=4 s; title yes, duration yes | harness can say yes; the old Songs path plays the clicked row |
| FIX-a0: row 0, `a.` playlist | fix | Playing t=3 s, `queueLength` 99 (99 of 99 resolved), moving by t=5 s, no banner; title yes, duration yes | PASS |
| FIX-aN: row 49, same playlist | fix | Playing t=2 s, `queueLength` 50 (rows 49 to 98), moving by t=4 s, no banner; title yes, duration yes; next track after the seek: `queuePosition` 1, title yes, duration yes against row 50 | PASS |
| FIX-i0: row 0, `i.` playlist | fix | Playing t=3 s, `queueLength` 88, moving by t=5 s, no banner; title yes, duration yes | PASS |
| FIX-iN: row 40, same playlist | fix | Playing t=3 s, `queueLength` 48 (rows 40 to 87), moving by t=5 s, no banner; title yes, duration yes; next track: `queuePosition` 1, title yes, duration yes against row 41 | PASS |
| fix-mixed-a, fix-mixed-i | fix | NOT RUN: the account has no playlist with both `a.` and `i.` rows (see below) | NOT RUN, not a pass |
| FIX-songs: Songs-list row 0 | fix | Playing t=3 s, `queueLength` 98, moving by t=5 s, no banner | PASS |
| FIX-err: `bramley play 0` | fix | banner in 41 samples is the underlying request error (MusicDataRequest 404); no Code=1, no MPMusicPlayerControllerErrorDomain | PASS |
| FIX-err control: same command | v0.7.1 source | banner in 41 samples has Code=1 and MPMusicPlayerControllerErrorDomain | control reproduces |

H1 verdict: CONFIRMED. The control reproduces on the all-`a.` playlist, and the fix plays the clicked row, then the next row in list order, on both the `a.` and the `i.` playlists.

VOID checks: other bramley processes during sampling 0 of every case; one bramley process was present when the run started and was gone before the first launch (the harness waits for a clear machine); output muted and built-in speakers confirmed before each launch; control reproduced; volume restored exactly; no bramley process left. Pacing: 6 playing starts and 2 track changes in 16 minutes, cases 75 s apart (the bar is at most 20 license actions per 10 minutes).

## The mixed-playlist gap

No mixed playlist exists in this account. All 35 playlists, every page loaded:
- 28 are all `a.` (4 of them over 100 rows: 107, 190, 249, 250).
- 5 are all `i.` (1 of them over 100 rows: 182).
- 2 are empty (0 rows).
- 0 have both kinds.

The first discovery printed "mixed 2" because its summary counted the 2 empty playlists (no row kinds) as mixed. The second pass loaded all pages of the 5 playlists over 100 rows and found each pure. The fix has no mixed-playlist run; mixed windows are covered by the unit matrix only (an `i.` row, an `a.` row with a catalog id and an `a.` row without, in one window). An account with a mixed playlist would need that run.

## Findings

1. **The fix-err banner is a raw Swift error dump** (the request error with status, code and response details). Correct per the bar, unhelpful for users. A follow-up, not part of this change.
2. **Not exercised in place:** the `resolved 0 of N asked` message and the partial-resolve skip (the request threw, so the first error was rethrown), and an `a.` row without a catalog id (every `a.` row in the cache carries one). The gate (fix 2) is shown only by the banner not being Code=1.
3. **Linux** queues the ids it always did; the unit matrix pins native=false, and the Linux suite is unchanged.

Evidence on the Air, `~/bramley-spike/s49/`: `run-cases-20261023-142153` (counted), `run-cases-20261023-135451` and `run-cases-20261023-133751` (earlier runs), `build3/`, `apps3/`, `apps2/ctl`, `gate/`.
