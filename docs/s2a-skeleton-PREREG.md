# S2a app skeleton: pre-registration (2026-10-10)

These checks and thresholds are fixed before anything is built or run. Each binary variant is built into its own `--target-dir`, so no run can measure a different binary by accident.

| Variant | Command | Path |
|---|---|---|
| release | `cargo build --release -p bramley` | `target/release/bramley` |
| dev | `cargo build --release -p bramley --features dev --target-dir target/dev` | `target/dev/release/bramley` |
| all | `cargo build --release --workspace --all-features --target-dir target/all` | `target/all/release/bramley` |

Every run uses weston headless with `--renderer=gl` under `sg render`, at 1100×720, through `scripts/app/run.sh`.

## Did it run (otherwise the run is VOID, not PASS)
- The status file exists, `glRenderer` is set and contains `AMD` or `radeonsi`, and does not contain `llvmpipe`.
- `framesPainted` > 0.
- One process in the tree.
- Playing runs only:
  - a temporary PipeWire null sink exists;
  - the app's output stream links only to that sink;
  - the default sink is the same before and after;
  - the sink is gone after the run.

## Checks
1. **Guard.**
   - `scripts/check-shipped-features.sh` exits 0 on release.
   - It exits 1 on dev, naming `app/dev`.
   - It exits 1 on all, naming `engine/sabotage` and `engine/diagnostics` (S1b AC3).
   - Release rejects `--play` with a non-zero exit.
2. **Idle screenshot (release).**
   - `--screenshot target/screens/idle.png` writes a 1100×720 PNG with more than one distinct colour.
   - The status has `engineStarted: false`.
3. **Playing screenshot (dev).**
   - `--play 6781024437 --screenshot target/screens/playing.png --after 30` writes a 1100×720 PNG with more than one colour.
   - The final status has `state: "Playing"`, `itemId: "6781024437"` and `position` > 15.
   - Position is the engine's native `currentTime`. Decrypted-seconds diagnostics are not linked into the app, by design.
4. **PSS idle (release).**
   - Measured with `scripts/playback/measure.py`: warmup 5 s, sample 30 s, 2 s interval.
   - Over the whole sample, `/proc/<pid>/task/*/comm` has no `bramley-player` thread.
   - **Sanity bar: median ≤ 100 MiB** (acceptance R2). Above 100 is reported as an R2 risk; fonts are not hacked to pass.
   - Reference: S0 measured fastframe at 85.85.
5. **PSS playing (dev).**
   - Same method: warmup 20 s, sample 60 s.
   - The status position is > 15 at the end, and still advancing.
   - **Recorded only; no bar.** S2c owns the 200 MiB gate.

## Sabotage and controls (each must give the named outcome)
- **A, no tokens:** dev `--play` with an empty `BRAMLEY_CONFIG_DIR` must FAIL check 3, with no `Playing` state, and the screenshot must show the sign-in banner.
- **B, software GL:** with `LIBGL_ALWAYS_SOFTWARE=1`, the renderer reads llvmpipe and the run must be VOID.
- **C, unit tests:** mutating the optimistic-hold comparison, and spawning in `App::new`, must each turn a named test red.

## Named false positives
- **A frozen position:** a "Playing" state with a frozen position is not playback. The position must advance between two status reads ≥ 2 s apart.
- **VRAM:** textures in VRAM do not show in PSS. This build has no textures beyond the font atlas.

## Addendum, 2026-10-10 (after review; the text above is unchanged)
- **Control C as run:** the lazy-start mutation spawned in `Playback::new`, not `App::new`. No unit test constructs `App`. Only the idle run's thread watch would catch a spawn there.
- **Control A collided with did-it-run:** the routing rule made a run with no stream VOID, so A could never read FAIL. Per acceptance.md (clarified the same day), the routing VOID now applies only once playback has started: Playing, with the position advancing. A must now read FAIL.
- **Check 3 is tightened:**
  - It FAILs on any non-null banner.
  - Position must be > 35 s (F4's bar, above a 30 s preview), not > 15.
  - The playing screenshot is taken with `--after 45`.
