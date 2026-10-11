# S7 search: results (2026-10-11, after review)

The pre-registration is `s7-search-PREREG.md`. These results are for the code at 4007a36, run as the `dev` build. The gated commit, 541173e, adds only the screenshot on top of it.

## F3 via the dev hook: PASS

This exercises the dev build's `--search` hook, which differs from acceptance F3 as written. CLI/MCP `bramley search` answers on stdout and does not fill the Search view, so acceptance F3's "the UI shows them" is still unwired and remains for S13 or a follow-up.

| Run | wall ms | in-app ms | songs / albums |
|---|---|---|---|
| live-20261011-103828 f3-0..4 | 404, 363, 323, 343, 323 | 404, 352, 327, 353, 329 | 10 / 7 each |

- **Median wall time:** 343 ms, against a bar of 2000.
- **Registered check:** ≥ 1 song and ≥ 1 album per run. It now runs as registered, from the per-section counts in status.
- **Every run:** `engineStarted` false, no stream, radeonsi.
- **Sabotage:** the search was pointed at `--api-base http://127.0.0.1:9`, with fixture tokens and a fixture developer-token URL, so no real token was sent. It ended in `error` with variant `Network`.

## Click plays: PASS

- **Outcome:** "Bohemian Rhapsody" (6781024437) played and the position advanced 2.82 s. The guard saw only the null sink.
- **Deviation from the pre-registration:** status has no result ids, so the run checked the played id against the known catalog id, not against the first result.

## Privacy

- The term `bohemian` appears in none of the F3 or sabotage logs.
- Status carries the error variant name, never its text.

## Compliance

- **Art after the storefront check.** The art guard rests on one fact: `Results` exist only after `Session::search` called `storefront()`, and that call returned 200 for this user token. The api test `a_search_only_returns_results_after_the_storefront_accepted_the_user_token` pins it. A 403 means no search request is sent, and so no results and no art.
- **Album and playlist rows** show art but open S8's placeholders, which cannot play yet. This is the same section D deferral S6 took for Home, and S8 owns closing it.
- **The Artists section is gone.** It was not in the plan.

## Gate: PASS (`s7-4`, 541173e)

| | s7-4 | main f35def2 (main-s7delta-3, earlier session) |
|---|---|---|
| R1 PSS / USS / RSS | 172.4 / 145.5 / 205.0 | 172.0 / 145.2 / 204.6 |
| R2 PSS / USS / RSS | 74.7 / 47.8 / 107.2 | 74.6 / 47.8 / 107.1 |
| reference before / after | 90.3 / 89.1 | 88.6 / 88.4 |

- **Verdict:** zone 3 PASS. The ballast fails in zone 1 and holds. `check-shipped-features` exits 0 on release and flags the dev build (`api/loopback`, `app/dev`).
- **Delta:** +0.4 / +0.1, smaller than the reference's drift of +1.7.
- **Not measured:** the gate never opens Search, so search memory is unmeasured.

## Budget

| | Cap | First pass | Review round |
|---|---|---|---|
| Source | 300 | +452 / -51 | +111 / -48 |
| Tests | 150 | 261 | +226 / -24 |

- **Fixtures:** 0. They are inline in the tests.
- **Harness:** 173 lines, +32 / -21 this round.
- **Over budget:** both rounds ran over their caps. Review-round tests are about double the ~120 the review allowed.

## Open

- **Unexplained "the search could not run".** One synthetic run at 5f32d38 failed with it. The status now records the error variant, and every run keeps its own log directory, so a recurrence can be diagnosed. It has not recurred.
