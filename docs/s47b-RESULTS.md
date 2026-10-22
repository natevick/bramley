# S47b: macOS wgpu/Metal renderer, results (2026-10-22)

Commit 98b8d5e, built on the Air from `git archive`. Pre-registration: `docs/s47b-PREREG.md`. Footprints are `footprint` MB, which are MiB. Raw runs: `~/bramley-spike/s47b/` on the Air.

## Verdicts

| Check | Result | Against the PREREG |
|---|---|---|
| Harness can fail (glow control) | 5 window-sized regions, 125.8 MiB normalized | FAILs criteria 1 and 2, as it must |
| M1 surface, ad-hoc, signed out | 57.9 MiB measured, 73.3 normalized, 2 regions, frames rising, Metal | PASS |
| M2 maximized | not attempted (needs Accessibility) | SKIPPED |
| M3 live resize | not run (needs a human) | NOT RUN |
| M4 probe | `authorized`, developerBytes 206, userBytes 244 | PASS |
| M4-idle, Developer ID, Home with 50 covers | **102.3 MiB**, 2 regions, 848 x 1062 pt | **FAIL by 2.3 MiB** (bar 100) |
| M4 playback, 2 min muted | 98.7 to 98.8 MiB, 2 regions, 8 valid samples | PASS (flat: max minus min 0.1 MiB) |

Report-only idle replicates (signed, same protocol as M1): 96.3, 94.2, 95.3 MiB, median 95.3, 2 regions each. The counted sample and the replicates came from different harnesses and differ by up to 8 MiB on the same build, so the 100 MB bar has about 5 MiB of headroom on the median and none on the worst launch.

## Details

- **M1:** the window was 360 x 1062 pt (macOS fills the free tile of Nate's tiled desktop), so the verdict reads the number normalized to 848 x 1062 pt (PREREG amendment). Launches: 58.0, 57.9, 57.9 measured; 73.4, 73.3, 73.2 normalized; `framesPainted` 49 to 56, 49 to 56, 49 to 55; `glRenderer` "Metal Apple M2"; no OpenGL category. S47's C2 read 74 to 78.
- **Signed out:** ad-hoc builds read `banner: true`, no covers, so M1 is the surface plus an empty Home. The budget verdict is the signed idle sample.
- **M4 playback:** sample 0 (2 s after `play`) was Paused at position 0, so it is void by the PREREG rule. Samples 1 to 8 were Playing with position 14.5 to 119.7 s. Footprint while playing was lower than the idle sample (98.8 against 102.3). Volume settings before and after were identical (`output volume:38, input volume:27, alert volume:33, output muted:false`). No process was left running.
- **Binary size:** 16,000,048 B unsigned (16,019,120 signed) against 12,387,664 for v0.7.0 glow (S47 A), +3.6 MB. Linux release binary unchanged at 23,622,016 B.
- **Clippy on the Air:** `cargo clippy -p bramley --all-targets -D warnings` fails with one error, `Backend::File` never constructed (`crates/app/src/credentials.rs:23`). Pristine main at 501ac27 fails identically, so it predates this change. `cargo test --no-run` builds.

## Left for a human

- **Live resize (M3):** drag the corner and the left edge of `~/bramley-spike/s47b/new/Bramley.app` against `~/bramley-spike/s47/C/Bramley.app` (with the jitter fix), slowly then fast, looking for the four signs in the PREREG.
- **Maximized window (M2):** zoom the window and read the IOSurface count. Expected 2 regions; the size is extrapolated, not measured.
- **The 2.3 MiB miss:** accept the median, or find what the counted run carried that the replicates did not.
