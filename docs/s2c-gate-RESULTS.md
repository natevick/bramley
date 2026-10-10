# S2c whole-app memory gate: results (2026-10-10)

Run against `s2c-gate-PREREG.md` with `scripts/acceptance.sh`, from a clean tree at c63ce73. The verdicts were computed by `gate.py` and are committed in `docs/results/s2c-gate/`. Evidence and logs stay in `target/acceptance/<run>/`.

## Verdicts

| Run | Gate | R1 PSS / USS / RSS | R2 PSS / USS / RSS | Reference PSS / USS (recorded 83.3) | Ballast |
|---|---|---|---|---|---|
| run1 | **PASS** | 160.2 / 141.9 / 184.3 | 63.0 / 44.7 / 87.0 | 80.2 / 61.9 (−3.1) | 55 MiB → 215.1, R1 FAIL on threshold only: **holds** |
| run2 | **PASS** | 161.0 / 142.7 / 185.0 | 63.8 / 45.5 / 87.8 | 79.5 / 61.2 (−3.8) | 54 MiB → 215.7, R1 FAIL on threshold only: **holds** |
| llvmpipe1 (`--llvmpipe`) | **VOID** | 190.5 (VOID: renderer) | 92.2 (VOID: renderer) | 79.5 / 61.2 | n/a; **llvmpipe control holds** |

- **Bars:** R1 ≤ 200 MiB and R2 ≤ 100 MiB.
- **Headroom:** about 39 MiB on R1 and 36 MiB on R2.
- **Validity:**
  - every sample read `AMD Radeon RX 5700 XT (radeonsi, navi10, …)`, 1 process and 50 covers;
  - the position advanced 15 s → 74 s across each 60-sample R1 window;
  - the engine was not started during R2;
  - the guard saw only the null sink and made about 100 clean polls before play;
  - the default sink (`…iec958-stereo`) was unchanged and the null sink was gone at the end.
- **Ballast:**
  - **USS:** the rise over shipped was 54.9 and 54.8 MiB, for 55 and 54 MiB requested.
  - **Feature check:** `check-shipped-features.sh` exited 1 on the ballast binary and named `app/gate-sabotage`; it exited 0 on the shipped one.
- **llvmpipe:** `LIBGL_ALWAYS_SOFTWARE=1` was set on the apps only, and they reported `llvmpipe (LLVM 21.1.8, 256 bits)`. The reference stayed on radeonsi, so the VOID comes from the renderer rule alone.
- **Guard after the `session()` refactor:** `covers-session.py guardtest` killed the app 0.27 s after its last clean poll, and the default sink was unchanged.
- **Screenshots:** from the dev build of the same commit, at `target/screens/acceptance-run{1,2}.png` (real art; for Nate only). They show 50 covers and Bohemian Rhapsody playing at about 0:41.

## Known limits
- **VRAM:** `vramDeltaMiB` is 0.0 because it spans only the R1 window, when the covers are already resident. It is not the covers' VRAM cost. S2b measured that at +4.0 MiB.
- **Shared pages:** PSS sits about 5 MiB below S2b's playing figures (164.7–169.5). The reference also reads 3–4 MiB under its recorded value, so the difference is session-level drift in shared pages, not an app change.
- **Not covered:** plan sabotage B (empty token dir), plan sabotage C (unreachable art host), and acceptance.md's "No user token" control. These belong to later harness slices.
