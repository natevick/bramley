# S7 search: results (2026-10-11)

Pre-registration: `s7-search-PREREG.md`. The code under test is e167745 (`dev` build). 04febc3, the gated commit, differs only in the harness.

## F3 via the dev hook: PASS

The dev build's `--search` hook, not acceptance F3 as written. The CLI/MCP `bramley search` path still answers on stdout and does not fill the Search view, so acceptance F3's "the UI shows them" is not wired. That is left for S13 (or a follow-up).

| Run (live-084636) | wall ms | in-app ms | results |
|---|---|---|---|
| f3-0..4 | 504, 443, 423, 463, 506 | 504, 454, 429, 454, 504 | 17 each |

- **Median wall time: 463 ms** (bar 2000).
- Every run: `engineStarted` false, no stream seen, radeonsi.
- Two earlier sessions, on code before the `Client::at` restriction, read medians of 322 and 685 ms.
- **Sabotage** (`--api-base http://127.0.0.1:9`, not listening): `search.state == "error"` after 25 ms.
- **Deviation:** the pre-registration asks for at least 1 song and at least 1 album. Status has no per-section counts, so the harness checked `results >= 2` in total. The private real screenshot shows both sections. That corroborates the result; it is not the registered check.

## Click plays: PASS

`--click-first-song` uses the same mapping as a row click.

- Status: `Playing`, itemId 6781024437, "Bohemian Rhapsody". Position advanced 2.82 s over 3 s.
- The guard saw only the null sink.
- **Deviation:** status has no result ids, so the run did not check that the playing id was the first result's id. It checked the id against the known catalog id instead.

## Privacy checks

- `bohemian` appears in none of the app logs from the 15 F3 runs, the sabotage runs, the play runs or the real-shot runs.
- No token appears in `git log -p main..HEAD`.

## Screenshots

- **real:** `target/screens/PRIVATE-real-art/s7/` (private). 13 art textures.
- **synthetic:** `docs/screens/s7/search-results-dark-1100x720.png`.
- **Unexplained failure:** one synthetic run at 5f32d38 ended in "the search could not run" after 25 ms. That message covers Config, Decode, Refused and a worker panic alike. It has not reproduced in 12 runs since.

## Gate: PASS (`s7-3`, 04febc3)

Same session. Clean `main` (f35def2) ran first, as `main-s7delta-3`:

| | main | S7 | delta |
|---|---|---|---|
| R1 PSS / USS / RSS | 172.0 / 145.2 / 204.6 | 171.0 / 144.1 / 203.7 | -1.0 PSS |
| R2 PSS / USS / RSS | 74.6 / 47.8 / 107.1 | 74.5 / 47.6 / 107.1 | -0.1 PSS |
| reference before / after | 88.6 / 88.4 | 89.1 / 89.2 | +0.5 / +0.8 |

- **Verdict:** both runs PASS in zone 3. The ballast FAILS in zone 1 and holds. `check-shipped-features` exits 0 on the release binary.
- **Earlier runs, not counted:**
  - `s7-1` failed on a dirty tree (an untracked screenshot).
  - `s7-2` (c5110c4) passed, but it predates the `Client::at` fix.
- **What the gate leaves out:** it never opens Search, so search art and the `search-art` thread are unmeasured. Art is 40 pt, at most 30 textures, about 6 KiB each at 1x.
