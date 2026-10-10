# S11 CLI control: pre-registration (2026-10-10)

The driver is `scratch/live/drive.py` (not committed; `scratch/` is gitignored), run through `scratch/live/run.sh` under `sg render`. These conditions were first written into that driver for run 1. This file fixes them, unchanged, before the re-run on the final binary. The round-2 additions are marked; they were added before the round-2 runs. The app is driven only through `bramley <command>`.

Binary: `cargo build --release -p bramley` → `target/release/bramley`. It must be fresh against HEAD.

## Did it run (otherwise VOID)
- `now` answers within 15 s. Its `glRenderer` contains `AMD`, not `llvmpipe`, and `framesPainted` > 0.
- A temporary PipeWire null sink is passed to the app as `PULSE_SINK`. Once the app is Playing, its only output stream links only to that sink. If not, the driver kills the app.
- The default sink is the same before and after, and the null sink is gone afterwards.

## Checks (signed in)
1. `play 6781024437` exits 0 with `seq` > 0. `now` reaches position > 35 with no banner and `applied >= seq` within 90 s.
2. PSS, USS and RSS: median of 10 one-second samples from `/proc/<pid>/smaps_rollup` while Playing. Recorded with no bar; this is not an R1 reading.
3. `pause`: `state` is Paused and `applied >= seq` within 1 s of the ack.
4. `seek 120`: `applied >= seq` and 119 ≤ position ≤ 130 within 5 s.
5. `toggle`: from Paused, `state` is Playing and `applied >= seq` within 5 s.
6. `queue add 6781079518`, then `next`: `itemId` is 6781079518, Playing, `applied >= seq` within 15 s.
7. `volume 40`: `applied >= seq` within 5 s.
8. The play ack carries `engine` 1 and `instanceGuarded` true (round 2).
9. Report only, no bar: `queue add 6781024437`, `pause`, then `next`. Record `state` and `itemId` 3 s after `next` is applied: does MusicKit start playback from Paused?

## Controls
- After the app exits, `now` exits 3.
- With an empty `BRAMLEY_CONFIG_DIR`, `play` exits 5 with "sign in". Five seconds later, `now` shows banner SignInNeeded, `engineStarted` false, Stopped, position null, and no `bramley-player` thread.
- In the same run:
  - a second no-argument launch exits 0 within 3 s, and the first pid still answers;
  - `search` exits 4;
  - `volume 500` exits 2.
- In the signed-out run, a second launch with `--status` exits 0 and says the flags are ignored (round 2).
- **Bad sink** (round 2): `PULSE_SINK` names a sink that doesn't exist. 25 s after `play`, the banner is an Error, and the app has no PipeWire output stream at all (so nothing reaches the default sink).
- `scripts/check-shipped-features.sh` exits 0 on release and 1 on `--features dev`.
