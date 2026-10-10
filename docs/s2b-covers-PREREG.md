# S2b cover grid: pre-registration (2026-10-10)

This is a measurement, not S2c's gate. Nothing here passes or fails the 200 MiB bar. It records what 50 covers cost, so S2c can pre-register against real numbers. These conditions are fixed before any S2b run.

## Change from the plan
- **Cells are 96 pt, not 160 pt.** At 160 pt only about 18 covers fit in 1100×720 above the player bar. R1 needs 50 visible. At 96 pt, 10 columns by 5 rows fit. AC1 (160 pt at 2× gives `320x320bb.jpg`) stays as a unit test of the template fill.
- **The leak build is `gate-sabotage`,** the plan's name (S1b, S2c). It keeps a copy of every decoded cover. It is S2c sabotage A only. It is **not** R3's 3 MiB-per-track leak, which is still unbuilt.

## Binaries, all built on this branch in one sitting, copied to `target/reference/`
| Name | What |
|---|---|
| `ref` | `bramley-main-4961e27`: main HEAD, the same-session reference. Recorded idle PSS 83.3 MiB (step 1, this branch's pin commit). |
| `pin` | `bramley-pin-e32305a`: the fastframe pin, no covers. |
| `s2b` | `bramley-s2b`: this story, release features. |
| `sab` | `bramley-s2b-sabotage`: `--features gate-sabotage`. |

## Environment and did-it-run (any miss makes that run VOID)
- weston headless `--renderer=gl` under `sg render`. `glRenderer` contains `radeonsi`, not `llvmpipe`.
- `framesPainted` > 0. Exactly 1 process.
- Before each launch, `bramley now` exits 3, so a held slot can't forward the launch and leave nothing to sample.
- `s2b` and `sab`: `texturesUploaded` = 50 and `coversFailed` null before sampling starts. `decodedBytesHeld` = 0 for `s2b`, and 1,843,200 for `sab` (50 × 96 × 96 × 4).
- **Reference drift:** if `ref`'s idle PSS median moves more than 5 MiB from 83.3, the whole session is VOID.

## Idle runs
- **Order:** interleaved `ref pin s2b sab`, three rounds, one at a time.
- **Sampling:** 20 samples at 1 s, after the covers are uploaded (or 12 s for `ref` and `pin`). Each run records PSS, USS and RSS medians.
- **Per-cover cost** = (median of the s2b medians − median of the pin medians) / 50, in PSS and in USS. This includes fixed costs: JPEG decoder code, the ureq agent, and three short-lived threads. It is a whole-feature cost spread over 50, not a marginal one.
- **Sabotage visibility:** the expected rise is 1.76 MiB. S2c's bar is ≥ 80 %, which is 1.41 MiB. Step 1's pin runs spread 2.7 MiB, so this is **marginal in advance**. Record sab − s2b in USS and PSS as DETECTED (≥ 1.41 in USS) or NOT DETECTED. It is not a verdict on the code.

## Playing runs
- **Binaries:** `pin` then `s2b`, twice each, interleaved. Driven only through `bramley play 6781024437`.
- **Audio safety, before any play:**
  - A PipeWire null sink is created with a unique name, and is confirmed to exist by exact `node.name` before launch and again right before `play`. The app gets it as `PULSE_SINK`.
  - A guard polls `pw-dump` every 0.25 s from launch to exit. It SIGKILLs the app if any of the app's output streams links to any other node, or if `pw-dump` fails.
  - The default sink is recorded before and after and must match. The null sink must be gone afterwards.
- **Sampling:** wait for Playing, `applied >= seq`, no banner and position > 15 s, then take 30 samples at 1 s. The position must advance across the window.
- **Report:** PSS, USS and RSS medians; covers-while-playing (s2b − pin); and `mem_info_vram_used` and `mem_info_gtt_used` before and after. Named false positive: textures in VRAM or GTT do not show in PSS, and GTT is system RAM.

## Guard self-test (must fail first)
One `s2b` play run uses a guard whose allowed sink name is wrong. The audio still goes to the null sink. The guard must kill the app within 2 s of the first link it sees. If it doesn't, every playing run is VOID.

## Screenshot
A separate `s2b` run with `--screenshot target/screens/s2b-covers-playing.png --after 45`, played through the CLI at about 5 s under the same guard. It must show 50 covers and the bar mid-song. Real album art stays out of the repo.
