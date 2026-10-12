# S9b library artists: pre-registration (2026-10-11)

Written before any live run of the S9b build. Thresholds do not move after data comes in.

## What runs

`scripts/app/s9b-session.py`, run under `sg render`. It reuses S9's harness pieces:
- weston headless `--renderer=gl` on the real GPU;
- a PipeWire null sink and the sink guard;
- an isolated copy of the config dir and a throwaway keyring service;
- the leak grep.

The binary is the `dev` build of the commit under test. The dev-only flags `--route artists`, `--scroll-end`, `--click-last` and `--click-row 0` stand in for a user who:
1. opens Artists;
2. scrolls to the end;
3. opens the last artist;
4. plays that artist's first song.

The last artist was picked before any data was seen, because it is the one only reachable by scrolling to the end.

For independent counts, the harness calls the documented endpoints itself, with Bramley's User-Agent and no Origin:
- `/v1/me/library/artists`;
- the artist's `albums` relationship;
- each album's `tracks` relationship.

Names go only to the gitignored `target/tmp/s9b/names.txt`, mode 0600, and the file is deleted in a `finally`. Evidence files hold counts, PSS and booleans, never names or ids. The status file passes through S9's key allowlist.

## Checks and pass conditions

**A1. Artists, scrolled to the end, then opened**
- `library.artists.loaded` equals `library.artists.total`, which equals the harness's `meta.total`.
- The artists list fetched more than one page if the total is over 100.
- Within 120 s, `route` becomes `Artist`. `--click-last` fires only once the list has no next page and its last row was on screen, so the route flip is the scrolled-to-the-end proof.

**A2. The artist view**
- `library.artistAlbums.loaded` equals the harness's album count for that artist.
- `library.artistSongs.loaded` equals the harness's track count summed over those albums, and `artistSongs.more` is false. Both are read after the play, within 60 s.
- `library.artTextures` is between 1 and 96: album art shows, within the LRU cap.
- `library.rowsBuiltMax` ≤ 30.

**A3. Playing a song from the artist view**
- Within 30 s of the view opening, `state` is `Playing` with `position` > 0.
- Within 10 s more, `itemId` is the first track of the artist's first album, as its library id or its catalog id.
- The guard never killed the app, and it saw the stream on the null sink only.
- `now` over the instance socket has a title, but the status file does not contain it.

**Did it run**
- radeonsi appears in every run's status.
- Frames are painted.

**Sabotage S1**
- The same run without `--scroll-end` must FAIL A1: `route` stays `Artists` for 60 s.
- If S1 passes, the route flip doesn't prove the end was reached.

**Leak grep**
- The library names (artists, albums, songs, at least 4 characters) never appear in this branch's diff, its commit messages or the runs' logs.
- A canary proves the grep can fail.
- Hits that are generic UI words also used as titles (`Home`, `Back`, `Untitled`) are listed by count and do not fail it.

## Report only (no bar)

- **R2-artists:** dev build, Artists route, never played. PSS median of 20 samples.
- **R1-artist:** PSS median of 20 samples while A3's song plays with the artist view open.
- **Gate:** `sg render -c 'bash scripts/acceptance.sh s9b-1'` must PASS, as acceptance.md defines. The delta against main is taken on the lead over the reference binary, from the same day.

## Named false positives

- A route flip to `Artist` without the end being reached would be a false A1. It can't come from `--click-last`, and S1 checks that it doesn't.
- `Playing` with an id from another artist means the queue started elsewhere. A3 fails it.
- `artTextures` > 0 cannot come from Home's covers: it counts only the library art cache, which the artist list never fills, since artists have no art.

## Amendment 1 (after live-1, before live-2; no bar changed)

- **R2-artists sampling:** live-1 sampled it in the scrolling run, which by then had played. From live-2 on, R2 is sampled only in the S1 run, which never plays. The S1 number in live-1 is the valid one.
- **Report only, added after live-1:** the last artist had a single album, so the album-by-album read was never exercised live. A `most-albums` run opens the artist with the most albums among the first 40 and scrolls its songs to the end.
  - `allAlbumsRead` means `artistAlbums.loaded` and `artistSongs.loaded` equal the harness's counts for that artist, and `artistSongs.more` is false within 120 s.
  - No bar: it is labelled post-registration.
