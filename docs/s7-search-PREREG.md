# S7 search: pre-registration (2026-10-11)

Fixed before any live run. Harness: `scripts/app/s7-session.py`. It runs one weston headless session (`--renderer=gl`, `sg render`) with a PipeWire null sink and the covers-session guard, against an isolated config and keyring. The binary is the `dev` build of the commit under test. Runs: `live`, then the `real` and `synthetic` shots.

## F3: search shows results in the UI

- **How it runs:** 5 launches. Each uses `--route search --search "bohemian rhapsody"`; the dev hook submits on the first tick.
- **Measurements:**
  - **wall:** from the first status that shows `framesPainted >= 1` to the first that shows `search.state == "results"`. Status is polled every 20 ms.
  - **in-app:** `search.ms`, from submit to reply.
- **PASS:** the median wall time is ≤ 2000 ms. Every run must reach `results` with at least 1 song and at least 1 album, `engineStarted` false, and the guard silent.
- **FAIL:** any run never reaches `results` within 15 s, or a run starts the engine (search alone must never start playback).

## Click plays

- **How it runs:** 1 launch with `--search "bohemian rhapsody" --click-first-song`. The guard starts before the app.
- **PASS:**
  - Within 15 s of the results, status shows `state == "Playing"` and `itemId` equals that run's first song id.
  - Position advances by at least 2 s between two samples.
  - The guard saw the null sink and nothing else (`streamsSeen == [sink]`), and it never fired.
- **VOID:** no stream was seen while position advanced. That proves nothing about the audio route.
- **FAIL:** anything else.

## Did it run

- Every run writes a status file. A missing file counts as FAIL.
- **Sabotage:** a run with `--search` pointed at a refusing `--api-base` (a loopback port that is not listening) must end in `search.state == "error"` and never reach `results`. That proves the F3 check can fail.

## Screenshots

- **real:** 1100×720, dark, with results and art. Written only to `target/screens/PRIVATE-real-art/s7/` and never committed.
- **synthetic:** a loopback fixture (flat-colour art and fixture tokens) serves storefront, search and art. One shot is published to `docs/screens/s7/`. The script refuses to write either set into the other's directory or under `docs/` directly.
