# S7 search: results (2026-10-11)

Pre-registration: `s7-search-PREREG.md`. Code under test: 5f32d38 (`dev` build). c5110c4 adds only the screenshot.

## F3: PASS

| Run (live-080603) | wall ms | in-app ms | results |
|---|---|---|---|
| f3-0..4 | 685, 706, 484, 424, 888 | 681, 707, 479, 429, 884 | 17 each |

- **Median wall time: 685 ms** (bar 2000).
- Every run: `engineStarted` false, no stream seen, radeonsi.
- An earlier run at e13ecdf read a median of 322 ms. The difference is network variance.
- **Sabotage** (`--api-base` at a closed loopback port): `search.state == "error"` after 25 ms. The F3 check can fail.

## Click plays: PASS

`--click-first-song` went through the same mapping as a row click.

- Status: `Playing`, `itemId` 6781024437, "Bohemian Rhapsody". Position advanced 2.82 s over 3 s.
- The guard saw only the null sink and never fired.
- **Deviation from the pre-registration:** status does not expose result ids, so the check that `itemId` equals the first result was not run as written. The id was confirmed against the known catalog id for Queen's recording, the same `SONG` covers-session uses.

## Screenshots

- **real:** `target/screens/PRIVATE-real-art/s7/`, private. 13 art textures, search ms 1512.
- **synthetic:** `docs/screens/s7/search-results-dark-1100x720.png`.
- **Intermittent failure:** one synthetic run at 5f32d38 ended in "the search could not run" after 25 ms. 11 runs since, some with a build that prints the raw error, have not reproduced it. The prime suspect is a transient secret-service error on the fresh isolated keyring, which `credentials.rs` maps to Config. That code is not S7's. Unproven.

## Gate: PASS (`s7-2`, c5110c4)

Same session, clean `main` f35def2 first:

| | main (main-s7delta-2) | S7 (s7-2) | delta |
|---|---|---|---|
| R1 PSS / USS / RSS | 172.8 / 145.9 / 205.3 | 170.2 / 143.3 / 202.9 | -2.6 PSS |
| R2 PSS / USS / RSS | 74.7 / 47.9 / 107.2 | 72.1 / 45.3 / 104.8 | -2.6 PSS |
| reference before / after | 88.7 / 88.7 | 89.7 / 89.8 | +1.0 |

- **Zone:** both runs are zone 3 and PASS. The ballast fails in zone 1, as it must. `check-shipped-features` exits 0.
- **What the deltas mean:** they are noise. The gate stays on Home and never opens Search, so search art and the `search-art` thread are not measured.
- **Search art size:** 40 pt textures, about 6 KiB each at 1x. A full result set holds at most 30.
