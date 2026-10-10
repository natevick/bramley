# S2a app skeleton: results (2026-10-10)

Run against `s2a-skeleton-PREREG.md`. Every run below used the final binaries, rebuilt after the last code change (the status `seq` field).

Conditions:
- **All runs:** weston used the GL renderer `AMD Radeon RX 5700 XT (radeonsi, navi10…)` and painted frames > 0. The exception is control B.
- **PSS runs:** one process.
- **SINK runs:** the default sink was unchanged and the null sink was gone afterwards.

| Check | Verdict | Evidence |
|---|---|---|
| 1. Guard | PASS | release exit 0; dev exit 1 `app/dev`; all-features exit 1 `app/dev engine/diagnostics engine/sabotage`; release `--play` → `unknown argument`, exit 1 |
| 2. Idle screenshot (release) | PASS | 1100×720, 113 sampled colours; `engineStarted: false`, `seq: 0` |
| 3. Playing screenshot (dev) | PASS | 1100×720; `Playing`, item 6781024437, position 27.98 s, advancing; `applied 2 = seq 2`, so this is the engine's state, not the optimistic one; routed only to the null sink |
| 4. PSS idle (release) | PASS | **81.1 MiB** median (max 81.7, 15 samples); 0 `bramley-player` threads in every sample |
| 5. PSS playing (dev) | recorded | **180.2 MiB** median (max 180.2, 30 samples); position 78.96 s, `applied 2 = seq 2`; routed only to the null sink |

## Sabotage and controls
- **A, empty `BRAMLEY_CONFIG_DIR`:**
  - Verdict **VOID**, carrying both FAIL reasons: not playing past 15 s, and the position did not advance.
  - The banner shows, and no player thread ran.
  - The PREREG contradicts itself here. Its routing did-it-run rule makes every run without playback VOID, so this sabotage can never read FAIL.
  - `acceptance.md` has the same collision ("no user token: F4 must fail" against "no stream: F4 VOID"). S2c and S13 should apply the routing VOID only once the app reached Playing.
- **A2, rejected user token (not pre-registered):**
  - Setup: the real developer token with a garbage user token.
  - Result: `SignInNeeded`, the banner, state `Stopped`, no stream.
  - Verdict VOID, for the same reason as A.
- **FAIL path (not pre-registered):** `--after 8` is routed correctly and is caught as **FAIL** at position 6.3 s. So the verdict can fail on playback, not only void.
- **B, `LIBGL_ALWAYS_SOFTWARE=1`:** the renderer reads `llvmpipe (LLVM 21.1.8, 256 bits)`, so the run is **VOID**.
- **C, mutations:** each turns its tests red, and restoring turns them green.

| Mutation | Fails |
|---|---|
| `applied + 1 >= seq` | the hold test |
| spawn in `Playback::new` | the three lazy-start tests |
| drop `snap.error.is_some()` | the sign-in release test |
| skip the `SetQueue` send | the mapping test and the hold test |

## Notes
- **Idle:** 81.1 MiB is under S0's fastframe figure of 85.85. This app draws a bar, not S0's 1,000-row list.
- **Playing:** 180.2 MiB, with no covers. S2c owns the 200 gate.
- **Fonts:** fastframe-fonts still reads the 25 fallback faces into the heap. S0's mmap fix is on no remote yet, so it is not pinned here.

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
