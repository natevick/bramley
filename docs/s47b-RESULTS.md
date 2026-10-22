# S47b: macOS wgpu/Metal renderer, results (2026-10-22)

Commit 98b8d5e, built on the Air from `git archive`. Pre-registration: `docs/s47b-PREREG.md`. Footprints are `footprint` MB, which are MiB. Raw runs: `~/bramley-spike/s47b/` on the Air.

## Verdicts

| Check | Result | Against the PREREG |
|---|---|---|
| Harness can fail (glow control, round 2) | 137.4 MiB, 5 window-sized regions, 848 x 1062 pt | FAILs criteria 1 and 2, as it must |
| M1 surface, ad-hoc, signed out, round 2 | 73.8, 73.3, 73.8 MiB (median 73.8), 2 regions, frames rising, Metal | PASS |
| M2 maximized | not attempted (needs Accessibility) | SKIPPED |
| M3 live resize | not run (needs a human) | NOT RUN |
| M4 probe | `authorized`, developerBytes 206, userBytes 244 | PASS |
| M4-idle, Developer ID, Home with 50 covers | **102.3 MiB**, 2 regions, 848 x 1062 pt | **FAIL by 2.3 MiB** (bar 100) |
| M4 playback, 2 min muted | 98.7 to 98.8 MiB, 2 regions, 8 valid samples | PASS (flat: max minus min 0.1 MiB) |

Report-only idle replicates (signed, same protocol as M1): 96.3, 94.2, 95.3 MiB, median 95.3, 2 regions each. The counted sample and the replicates came from different harnesses and differ by up to 8 MiB on the same build, so the 100 MB bar has about 5 MiB of headroom on the median and none on the worst launch.

## Details

- **M1:** round 1 got a 360 x 1062 pt window (macOS fills the free tile of Nate's tiled desktop), which voids it as first registered; its normalized numbers (73.4, 73.3, 73.2; control 125.8) are report-only. Round 2 got 848 x 1062 and counts: new build 73.8, 73.3, 73.8 MiB measured, `framesPainted` 49 to 55, 49 to 56, 49 to 56, `glRenderer` "Metal Apple M2", no OpenGL category; glow control 137.4 MiB with 5 regions. The round 1 normalization predicted 73.3 and the measurement was 73.3 to 73.8. S47's C2 read 74 to 78.
- **Signed out:** ad-hoc builds read `banner: true`, no covers, so M1 is the surface plus an empty Home. The budget verdict is the signed idle sample.
- **M4 playback:** sample 0 (2 s after `play`) was Paused at position 0, so it is void by the PREREG rule. Samples 1 to 8 were Playing with position 14.5 to 119.7 s. Footprint while playing was lower than the idle sample (98.8 against 102.3). Volume settings before and after were identical (`output volume:38, input volume:27, alert volume:33, output muted:false`). No process was left running.
- **Binary size:** 16,000,048 B unsigned (16,019,120 signed) against 12,387,664 for v0.7.0 glow (S47 A), +3.6 MB. Linux release binary unchanged at 23,622,016 B.
- **Clippy on the Air:** `cargo clippy -p bramley --all-targets -D warnings` fails with one error, `Backend::File` never constructed (`crates/app/src/credentials.rs:23`). Pristine main at 501ac27 fails identically, so it predates this change. `cargo test --no-run` builds.

## Left for a human

- **Live resize (M3):** drag the corner and the left edge of `~/bramley-spike/s47b/new/Bramley.app` against `~/bramley-spike/s47/C/Bramley.app` (with the jitter fix), slowly then fast, looking for the four signs in the PREREG.
- **Maximized window (M2):** zoom the window and read the IOSurface count. Expected 2 regions; the size is extrapolated, not measured.
- **The 2.3 MiB miss:** accept the median, or find what the counted run carried that the replicates did not.

## Round 2 (commit 215011f: portal detect off the startup path, `Backend::File` gated)

Registered under the 13:35 PREREG amendment, before measuring.

| Check | Result |
|---|---|
| M4-idle, Developer ID, Home with 50 covers, 848 x 1062 pt, 45 s, one registered sample | **96.3 MiB**, 2 regions. PASS (bar 100) |
| M1 again, ad-hoc | no counted set: 10 launches all opened at 1700 x 1062 (the desktop changed), report-only normalized 78.2 to 79.4 |
| Signed replicates | not taken (off-size) |

- Round 1's 102.3 MiB stays recorded as the first registered sample of the first build. Round 2 is a different build (a 3 to 4 s startup stall removed, no change to what the window or covers hold), so both numbers stand and the bar was not moved.
- Void launches before the counted one were voided for window size only: 562, 1199 and 891 pt wide, and a 1710 x 1073 full-screen window (138.1 MiB, which matches the budget's 5K extrapolation of about 2 x 59 MB of surface).
- `decodedBytesKept` reads 0 in the idle sample. It is a `gate-sabotage` accounting field (50 x 200 x 200 x 4 = 8,000,000 B only in that build); the shipped build keeps no decoded pixels, and `fifty_covers_become_textures_and_nothing_decoded_stays_once_a_frame_ends` already asserts it.
- Mac clippy `-D warnings` is clean and `cargo test -p bramley --bin bramley` passes (310) on the Air.
- Signed binary 16,035,568 B.

