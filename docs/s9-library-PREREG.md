# S9 library lists: pre-registration (2026-10-11)

Written before any live run of the S9 build. Thresholds do not move after data comes in.

## What runs

`scripts/app/s9-session.py`, run under `sg render`. It uses weston headless `--renderer=gl` on the real GPU, a PipeWire null sink, and the sink guard from `covers-session.py`. It runs against an isolated copy of the config dir and a throwaway keyring service, the same way the acceptance harness does. The binary is the `dev` build of the commit under test. The dev-only flags `--route playlist:<id>`, `--scroll-end <s>` and `--click-last` stand in for a user scrolling and clicking.

To pick the largest playlist and get independent totals, the harness calls the documented library endpoints itself (compliance rule 4) with Bramley's own User-Agent and no Origin (rule 5). It writes library names only to the gitignored `target/tmp/s9/names.txt`, for the leak grep. Evidence files hold counts, PSS and booleans; they never hold names or ids. The status file passes through a key allowlist, since `title`/`artist` turn into library names once a library track plays.

## Checks and pass conditions

**L1. The largest playlist, scrolled to the end**
- The playlist has ≥ 100 tracks.
- `library.tracks.loaded` equals `library.tracks.total`, which equals the harness's own `meta.total`.
- `library.shownTo` equals `loaded`, so the last row was on screen.
- `library.rowsBuiltMax` ≤ 30 (visible rows plus the margin, at 1100×720).
- `library.artTextures` ≤ 96, the LRU cap.

**L2. Playing a track from the end of that playlist**
- Within 30 s of the end click: `state` is `Playing` with `position` > 0.
- The guard never killed the app, and it saw the stream linked to the null sink only.
- The snapshot's `itemId` is the clicked (last) track, as either its library id or its catalog id.

**L3. Songs, scrolled to the end**
- `library.songs.loaded` equals `library.songs.total`, which equals the harness's `meta.total`.
- `shownTo` equals `loaded`.
- `rowsBuiltMax` ≤ 30.
- `artTextures` ≤ 96.
- PSS after minus PSS before is ≤ 10 MiB. Each side is the median of 10 one-second samples: before scrolling starts, and after the end is reached.

**Did it run**
- radeonsi appears in the status for every run.
- Frames are painted.
- L1 and L3 each fetched more than one page.

**Sabotage S1**
- The same Songs run without `--scroll-end` must FAIL L3's end condition (`shownTo < loaded` or `loaded < total`).
- If S1 passes, the harness cannot see a list that never reached its end, and L3 counts for nothing.

## Report only (no bar)

- **R2-library:** dev build, Songs route, never played. PSS median of 20 samples, next to a dev-build Home control in the same session.
- **R1-library:** PSS median of 20 samples while L2's track plays with the playlist route open.
- **Gate:** `sg render -c 'bash scripts/acceptance.sh s9-1'` must PASS, as acceptance.md defines. R1/R2 are reported against main measured on the same day. The delta is taken on the lead over the reference binary, because absolute PSS drifts between sessions.

## Named false positives

- Textures live in VRAM/GTT, not PSS. A flat PSS delta does not prove that the art is dropped. The LRU test checks `tex_manager().num_allocated()`; live, `artTextures` ≤ 96 is the check.
- `loaded == total` alone can pass if the view never scrolled. That is why `shownTo == loaded` is required, and S1 proves the harness can tell the difference.
- A `Playing` state with an id other than the clicked track's means the queue started somewhere else. L2 fails it.

## Amendment 1 (before gate run s9-3; no threshold changed)

Gate runs s9-1 and s9-2 FAILed only on the ballast control: R1 and R2 PASSed, and the ballast's USS rise beat its size by 3.3 MiB against a 3.0 tolerance. Both ran builds from before later fixes, so neither can stand for HEAD.

**Stopping rule:** `s9-3` runs once, at the final HEAD, on a clean tree.
- If its ballast control holds, the report gives s9-1 FAIL, s9-2 FAIL and s9-3 PASS together, with the per-run excess.
- If it misses on the rise again, a `main` gate runs right after it in the same session. Then the work stops and goes back as a blocker. There is no fourth S9 run.

RISE_TOLERANCE is not changed here; a change would be Nate's call.

**Checks added after live-2:**
- The live check of L2 waits up to 10 s for a now-playing item before comparing it. This is a procedure fix: one diagnostic run read the status before MusicKit had set an item.
- A separate click check, labelled post-registration, covers the Songs and Albums click paths. Bar: Playing with the position advancing, no banner, null sink only, and the clicked song (or one of the clicked album's tracks) playing.
