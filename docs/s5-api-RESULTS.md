# S5 Apple Music API client: results (2026-10-10)

Pre-registration: `docs/s5-api-PREREG.md`. Driver and evidence: `scratch/s5/` (gitignored): `live.sh`, `runs/{real,final,tip,sabotage}/`, `runs-s3filter.txt` and `mutation-log.txt`. The token-bearing trace logs were deleted after decoding. The binary was a fresh release build of the branch tip. It ran on weston headless `--renderer=gl` with the AMD Radeon RX 5700 XT (radeonsi).

## Checks

| # | Check | Result |
|---|---|---|
| 1 | F3 search ×5, `tip` run (the committed binary) | PASS. Times 0.424, 0.382, 0.410, 0.423 and 0.379 s; median **0.410 s**; all 5 exit 0 and include 6781024437. Earlier builds gave medians of 0.309 s (`real`) and 0.389 s (`final`). |
| 2 | Library smoke test (`--cfg bramley_tokens`), run twice | PASS. 34 playlists. The largest, playlist A, has `meta.total` 250 and 250 tracks listed over 3 pages. Library albums total 376; songs total 4481. |
| 3 | `check-shipped-features.sh target/release/bramley` | PASS, exit 0 |
| 4 | No token in a `RUST_LOG=trace` log | **FAIL, pre-existing on main**: see below |
| Control | Corrupted developer token | All 5 searches exit 1 with "Bramley's developer token was rejected"; no token appears in the output |

## Check 4: tokens leak through ureq's trace hex dumps

- A plaintext grep of app.log (35,124 lines, 27,799 of them ureq/rustls) finds no token.
- But the `ureq_proto::util` TRACE lines hex-dump the raw request bytes. Decoding them recovered **both** the developer token and the user token. That log has since been deleted.
- This is not new on this branch: main's covers storefront check sends the same headers through the same dump path. A plaintext grep, as S11's log check used, cannot catch it.
- **S3's filter fixes it (executed).** S3's `logging()`/`withhold_wire_dumps` hunk and the `log` dependency were applied to this branch as a throwaway, and the trace run was repeated (`runs-s3filter.txt`).
  - Not VOID: 34,919 lines, of which 27,801 are ureq/rustls and 10 name `catalog/us/search`.
  - All 27,000 `ureq_proto::util` lines read "HTTP bytes withheld".
  - The decoded dumps are 0 bytes, and neither token appears anywhere in the log.
  - Searches still took 0.37–0.46 s.
  - The hunk was then reverted, so it is not in this commit.
- **S5 must not ship to users before S3 merges, unless that filter lands separately first.**

## Found live

- An empty library playlist answers `404 "No related resources found for tracks"`. `playlist_tracks` now maps that to an empty page with total 0, and a fixture test pins it.
- Library `next` links drop `limit`, so the client re-adds `limit=100` on library paths. The live run paged 250 tracks in 3 requests.

## Mutations (each one fails its named test)

20 mutations, all killed on the committed code. The full set was rerun after the last code change, and none was skipped for a missing pattern. Round 1 had one survivor (ids unchecked). It was fixed by checking the id before the storefront call and adding album ids to the test. The test that kills each one:

| Test | Mutations it kills |
|---|---|
| search | user token sent on every path; no Bramley UA |
| library_pages | `limit` dropped from `next`; unknown kinds fail the page |
| library_pages and an_album | `all()` stops after one page |
| a_redirect | redirects followed |
| an_oversized | no body limit |
| statuses_map | detail not redacted; 401 untyped; 403 untyped |
| a_429 | no retry cap; Retry-After ignored |
| only_documented | allowlist open; ids unchecked |
| an_empty_library | the empty playlist is an error |
| signed_out | the call is still sent |
| the_worker | the worker drops the result |
| app relay | search not handed to the worker; an error read as results; `outcome` rejects results |

## Not established

- The library walk ran through the client, not through the running app. The app exposes only `search` until S9 and S12.
- 429 handling was tested on fixtures only; no real rate limit was hit.
- The 429 budget (1 s, at most 2 retries) is per request. A search makes 2 requests, so its worst case is about 2 s of waiting. That still fits inside the CLI's 3 s deadline.
- `docs/s5-api-PREREG.md` was written before the first live run, but it was committed afterwards, together with this file.
