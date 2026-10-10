# S2b cover grid: results (2026-10-10)

Run against `s2b-covers-PREREG.md`, in one weston session (`scratch/s2b/session.py`, gitignored, evidence in `scratch/s2b/session1/`). Binaries are in `target/reference/`, built at d7596da.

## Validity: VALID
- Every run reports `AMD Radeon RX 5700 XT (radeonsi…)`, frames > 0, and 1 process (0 children).
- **Reference:** `ref` idle median 83.0 MiB, 0.3 from its recorded 83.3 (bar 5).
- **Covers:** `s2b` reached `texturesUploaded` 50 with `decodedBytesHeld` 0 and `coversFailed` null in every run. `sab` held 1,843,200 bytes.
- **Playing:** position 15 → 45 s across each 30-sample window, Playing, no banner.
- **Audio:** every stream linked only to the null sink; the default sink (`…iec958-stereo`) was unchanged; the null sink was gone afterwards.
- **Guard self-test:** with a wrong allowed sink name, the guard SIGKILLed the app on the first poll that saw its link (0.0 s); the default sink was unchanged.

## Idle, MiB (PSS / USS / RSS), interleaved
| Round | ref | pin | s2b | sab |
|---|---|---|---|---|
| 1 | 82.6 / 60.2 / 110.7 | 63.1 / 40.7 / 91.1 | 68.1 / 45.7 / 96.3 | 69.4 / 47.0 / 97.6 |
| 2 | 83.1 / 60.7 / 111.2 | 63.1 / 40.7 / 91.2 | 68.5 / 46.1 / 96.7 | 69.1 / 46.7 / 97.4 |
| 3 | 83.0 / 60.5 / 111.0 | 63.0 / 40.7 / 91.1 | 68.1 / 45.6 / 96.3 | 69.3 / 46.9 / 97.5 |

- **Per-cover cost:** (68.1 − 63.1) / 50 = **0.10 MiB PSS** (USS also 0.10). That is about 102 KiB per cover, against 36 KiB of RGBA that is not held. Anonymous memory rises 3.0 MiB; the rest is file-backed. This includes the fixed costs named in the PREREG.
- **Sabotage visibility: NOT DETECTED.** sab − s2b in USS is 1.3, 0.6 and 1.3 (median 1.3), below the 1.41 bar; the expected rise is 1.76. Unverified explanation: s2b's freed decode buffers stay in the allocator, so part of what sab keeps is already counted in s2b. **For S2c:** at 96 pt, sabotage A as planned (≥ 80 % of 50·w·h·4) is not reliably measurable.

## Playing, MiB (PSS / USS / RSS)
| Run | pin | s2b |
|---|---|---|
| 1 | 162.8 / 140.4 / 190.9 | 166.2 / 143.8 / 194.4 |
| 2 | 163.3 / 140.9 / 191.6 | 166.7 / 144.2 / 194.8 |

- **Whole app while playing with 50 covers visible: 166.2–166.7 MiB PSS** (USS 143.8–144.2).
- **Covers while playing:** +3.4 MiB PSS, +3.4 USS.
- The reference was measured idle only, so there is no same-session playing figure for main. S11's 183.2 came from another session.

## GPU (named false positive)
`mem_info_vram_used` rose 21.4 MiB per `s2b` run against 17.4 per `pin` run: **+4.0 MiB VRAM** for 50 textures, the same in every run. `mem_info_gtt_used` moved 12–20 MiB in both binaries, so the covers can't be told apart from noise there. Neither shows in PSS.

## Screenshot
`target/screens/s2b-covers-playing.png` (gitignored, real art; for Nate only): 1100×720, 50 covers in 10 × 5, and the bar on Bohemian Rhapsody at 0:42. The status says Playing, applied 2 = seq 2.

## Re-run
```
cargo build --release -p bramley --features gate-sabotage && cp target/release/bramley target/reference/bramley-s2b-sabotage
cargo build --release -p bramley && cp target/release/bramley target/reference/bramley-s2b
sg render -c "python3 -I scratch/s2b/session.py --out scratch/s2b/session1 $(cat scratch/s2b/runs.txt)"
```
