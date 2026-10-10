# S2a app skeleton: results (2026-10-10)

Run against `s2a-skeleton-PREREG.md` on the code at `feat(app)` (rebuilt after the final edit). Every run had weston on the GL renderer `AMD Radeon RX 5700 XT (radeonsi, navi10…)`, one process, frames > 0, the default sink unchanged and the null sink gone afterwards.

| Check | Verdict | Evidence |
|---|---|---|
| 1. Guard | PASS | release exit 0; dev exit 1 `app/dev`; all-features exit 1 `app/dev engine/diagnostics engine/sabotage`; release `--play` → `unknown argument`, exit 1 |
| 2. Idle screenshot (release) | PASS | 1100×720, 113 sampled colours, `engineStarted: false` |
| 3. Playing screenshot (dev) | PASS | 1100×720; `Playing`, item 6781024437, position 27.98 s, advancing; stream routed only to the null sink |
| 4. PSS idle (release) | PASS | **81.6 MiB** median (max 81.6, 15 samples); 0 `bramley-player` threads in every sample |
| 5. PSS playing (dev) | recorded | **180.0 MiB** median (max 180.4, 30 samples); position 78.8 s at the end, routed only to the null sink |

## Sabotage and controls
- **A, empty `BRAMLEY_CONFIG_DIR`:**
  - Verdict **VOID**, with both FAIL reasons ("not playing 6781024437 past 15 s", "position did not advance").
  - VOID wins because no stream ever reached the sink. The PREREG expected FAIL; either way, playback is not a PASS.
  - The screenshot shows the sign-in banner; `engineStarted` stayed false.
- **A2, rejected user token (unregistered):** a real developer token with a garbage user token gives `SignInNeeded` and the banner, with state `Stopped`.
- **B, `LIBGL_ALWAYS_SOFTWARE=1`:** renderer `llvmpipe (LLVM 21.1.8, 256 bits)`, so the run is VOID.
- **C, mutations:**
  - `applied + 1 >= seq` fails `shown_state_holds_until_a_snapshot_has_applied_the_command` ("a stale snapshot undid the action").
  - Spawning in `Playback::new` fails the three lazy-start tests.

## Notes
- **Idle:** 81.6 sits under S0's fastframe 85.85. The app draws a bar, not S0's 1,000-row list.
- **Playing:** 180.0 has no covers. S2c owns the 200 gate, with 20 MiB of headroom left for art.
- **Fonts:** fastframe-fonts still reads the 25 fallback faces into the heap. S0's mmap fix (local branch `mmap-fallback-faces`) is on no remote yet, so it is not pinned here.

## Re-run
```
cargo build --release -p bramley
cargo build --release -p bramley --features dev --target-dir target/dev
sg render -c "scripts/app/run.sh idle-shot target/release/bramley --screenshot target/app-runs/idle-shot.png"
sg render -c "SINK=1 scripts/app/run.sh playing-shot target/dev/release/bramley --play 6781024437 --after 30 --screenshot target/app-runs/playing-shot.png"
sg render -c "MEASURE='5 30' scripts/app/run.sh pss-idle target/release/bramley"
sg render -c "SINK=1 MEASURE='20 60' scripts/app/run.sh pss-playing target/dev/release/bramley --play 6781024437"
python3 -I scripts/app/s2a_verdict.py <name> idle|playing
```
