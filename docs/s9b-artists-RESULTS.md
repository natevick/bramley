# S9b library artists: results (2026-10-11)

Pre-registration: `docs/s9b-artists-PREREG.md`, including amendment 1. Live harness: `scripts/app/s9b-session.py`, run under `sg render`.

**Environment.**
- weston headless `--renderer=gl` on the AMD Radeon RX 5700 XT (radeonsi).
- A PipeWire null sink, with the sink guard on every run that could play.
- An isolated config and keyring.
- After each session the default sink was unchanged and the null sink was gone.

**Evidence.** It stays on the dev box under `target/s9b-runs/` and `target/acceptance/` of the S9b worktree.
- The per-run status files are deleted at the end of each run.
- Names lived only in `target/tmp/s9b/names.txt` (mode 0600, 150 strings), which the session deleted in its `finally`.
- The grep covered the branch diff, the commit messages and 4 app logs: **0 hits**, and the canary showed the grep can fail.
- A wider pre-commit grep against 4,729 library strings found only ordinary English and UI words that some title shares, in text written for this branch.

## API (documented only)

| Path | Apple documentation |
|---|---|
| `/v1/me/library/artists` | `applemusicapi/get-all-library-artists` |
| `/v1/me/library/artists/{id}/albums` | `href` of `libraryartists/relationships-data.dictionary/libraryartistsalbumsrelationship`; Apple's own `include=albums` reply returns exactly this href |
| `/v1/me/library/albums/{id}/tracks` | `href` of `libraryalbums/relationships-data.dictionary/libraryalbumstracksrelationship`, "the relative location to fetch the relationship directly" |

- **Artwork:** `LibraryArtists.Attributes` documents only `name` and `inFavorites`, so the artists list shows names only.
- **Songs:** library artists have no songs relationship, so an artist's songs are the tracks of their albums.
- **Ruled out:** `get-multiple-library-albums` with `include=tracks`. Apple answers 400 (code 40005): include works only with a single-resource fetch.

## Live checks, final: live-2 at 9e8b2b7, PASS

| Check | Result |
|---|---|
| A1 Artists to the end, then open the last | loaded 137 = total 137 = harness total, 2 pages; route flipped to `Artist`. **PASS** |
| A2 the artist view | albums 1 = harness; songs 15 = harness, `more` false; art textures 11 (cap 96); rows built max 15. **PASS** |
| A3 play the first song | Playing with the position advancing; the playing item is the clicked song; `now` has a title, the status file doesn't; guard silent, null sink only. **PASS** |
| S1 sabotage (no `--scroll-end`) | loaded 100 of 137, route stays `Artists`: A1 **FAIL**, as required |
| most-albums (post-registration, report only) | 14 albums and 150 songs = harness counts, `more` false: every album was read in turn |
| Did it run | radeonsi in every status; frames painted |

live-1 passed the same A1–A3 and S1. It ran a build of the working tree that 9e8b2b7 later committed unchanged. Amendment 1 explains why its R2 field is not used.

## Memory (report only, dev build)

| Reading | live-1 | live-2 |
|---|---|---|
| Artists route, never played (R2-artists, from the S1 run) | 73.4 | 58.1 |
| Artist view, playing (R1-artist) | 173.6 | 147.6 |

The S13 worktree had its own weston and Bramley running during live-2, so sharing pulled every PSS reading down. These numbers are not comparable across sessions; the gate below is the memory claim.

## Acceptance gate

| Run | Commit | R1 PSS / USS / RSS | R2 PSS / USS / RSS | Ref before / after | Gate |
|---|---|---|---|---|---|
| s9b-1 | 9e8b2b7 | — | — | missing | **FAIL** (harness: the worktree had no `target/reference` binary) |
| main-s9b-1 | ab13c22 (main) | — | — | drift −14.5 | **VOID** (S13 ran gate-like sessions alongside) |
| **s9b-2** | **9e8b2b7 (clean)** | **172.5 / 145.5 / 205.1** | **75.5 / 48.5 / 108.1** | 90.5 / 90.0 | **PASS** |
| main-s9b-2 | ab13c22 (clean), right after | 172.9 / 145.9 / 205.5 | 75.0 / 48.1 / 107.6 | 89.3 / 88.9 | PASS |

- **Before s9b-2:** the reference binary was copied in from the main checkout; its sha256 prefix `1a72eb4b2880ac5a` matches acceptance.md.
- **Ballast control:** both runs hold it (zone 1, rise 71.4 / 71.5 against 70).
- **Delta against main,** on the lead over the reference before:
  - R1: −1.6 MiB (82.0 against 83.6);
  - R2: −0.7 MiB.
  - USS: R1 −0.4 and R2 +0.4.
  - This is within session noise: no measurable cost.
- **`check-shipped-features.sh`:** exits 0 on the release binary.

## Tests

- **Counts:** app 133 (+5) with `dev`, api 19 (+1), engine 105; clippy is clean with and without `dev`.
- **Mutants:** 18 targeted mutants, 18 killed, re-run at 9e8b2b7 in a separate worktree. They cover:
  - paging: the album cursor, its continuation, the tick list, songs waiting for complete albums, and the reset on an artist change;
  - virtualization;
  - names only;
  - routing: click, sidebar order, parse;
  - click → Action: album, songs in context;
  - the top-up owner check;
  - the signed-out guard;
  - no names in status;
  - the API allowlist and the empty relationship.
