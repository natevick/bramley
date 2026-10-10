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

- **Per-cover cost:** (68.1 − 63.1) / 50 = **0.10 MiB PSS** (USS also 0.10). That is about 102 KiB per cover, against 36 KiB of RGBA that is not held. This includes the fixed costs named in the PREREG.
- **Where it goes** (a follow-up session, `scratch/s2b/session2/`, 12 s settle for both, Pss summed over `/proc/PID/smaps`). pin and s2b were 63.3 and 68.5, then 63.2 and 68.7. Against pin, s2b adds:
  - **+1.9 MiB of binary code pages**, file-backed but private;
  - **+3.3 MiB of non-heap anonymous mappings.** These are most likely the decoder threads' malloc arenas, which keep pages after the decoded buffers are freed (unverified);
  - **+0.2 MiB of heap.**
  - `/dev/dri` mappings carry 0 PSS in both.
- **Sabotage visibility: NOT DETECTED.** sab − s2b in USS is 1.3, 0.6 and 1.3 (median 1.3), below the 1.41 bar; the expected rise is 1.76. Unverified explanation: the same arena retention means part of what sab keeps is already counted in s2b. **For S2c:** at 96 pt, sabotage A as planned (≥ 80 % of 50·w·h·4) is not reliably measurable.

## Playing, MiB (PSS / USS / RSS)
| Run | pin | s2b |
|---|---|---|
| 1 | 162.8 / 140.4 / 190.9 | 166.2 / 143.8 / 194.4 |
| 2 | 163.3 / 140.9 / 191.6 | 166.7 / 144.2 / 194.8 |

- **Whole app while playing with 50 covers visible: 166.2–166.7 MiB PSS** (USS 143.8–144.2).
- **Covers while playing:** +3.4 MiB PSS, +3.4 USS.
- The reference was measured idle only, so there is no same-session playing figure for main. S11's 183.2 came from another session.

## GPU (named false positive)
`mem_info_vram_used` rose 21.4 MiB per `s2b` run against 17.4 per `pin` run: **+4.0 MiB VRAM** for 50 textures, the same in every run. `mem_info_gtt_used` moved 12–20 MiB in both binaries, so the covers can't be told apart from noise there. The `/dev/dri` mappings carry 0 PSS, so neither shows in PSS.

## Token leak check
- **Setup:** an `s2b` run under `RUST_LOG=trace` (`scratch/s2b/trace/`).
- **It ran:** the log holds 24,882 ureq lines, 42 rustls lines and 6 lines naming api.music.apple.com.
- **Result:** 0 hits for either token, 0 JWT-shaped strings and 0 `authorization` mentions. ureq prints the chart request with `"<NOTICE>": "1 HEADERS ARE REDACTED"`.
- **Scope:** Bramley's logger writes to stderr only (no `.file()`), and every run's stderr is in its `.log`.
- **Same grep over all of `scratch/s2b/`:** 0.

## Screenshot
`target/screens/s2b-covers-playing.png` (gitignored, real art; for Nate only): 1100×720, 50 covers in 10 × 5, and the bar on Bohemian Rhapsody at 0:42. The status says Playing, applied 2 = seq 2.

## Round 2: signed-in gate and decode limits (a99e0a0)
- **What changed:** covers are fetched and shown only when signed in, and decoding runs under `image::Limits`.
- **Harness:** `scripts/app/covers-session.py` with `scripts/app/covers-runs.txt`, in one session. Binary sha256 prefix a6b6b539ce503f52.
- **Verdicts:** `docs/results/s2b-covers/r2/*.verdict.json` (the session summary was copied by hand as `session.verdict.json`; from round 3 the script writes that name itself). They hold no logs, no art URLs and no tokens.
- **Validity:** VALID on the same did-it-run rules. `ref` read 83.0 / 83.1 / 82.9.

| Check | Result |
|---|---|
| Guard self-test | killed on the first link it saw (0.0 s); default sink unchanged |
| Signed out, developer token present (`s2b-signedout`) | banner SignInNeeded, `coversRequested` 0, `texturesUploaded` 0, engine not started: **PASS** |
| The same control on the round-1 binary (`r1-signedout`) | `texturesUploaded` 50: the control **can fail** |
| Real art under the limits | `coversRequested` 51, 50 uploaded, `coversFailed` 0 |

**Idle, MiB (PSS / USS):**

| Round | ref | pin | s2b | sab |
|---|---|---|---|---|
| 1 | 83.0 / 60.6 | 63.3 / 40.9 | 68.0 / 45.6 | 69.5 / 47.1 |
| 2 | 83.1 / 60.7 | 63.1 / 40.6 | 68.0 / 45.6 | 69.7 / 47.3 |
| 3 | 82.9 / 60.5 | 63.1 / 40.7 | 67.9 / 45.5 | 69.5 / 47.1 |

- **Per cover:** (68.0 − 63.1) / 50 = **0.10 MiB**, unchanged.
- **Sabotage:** sab − s2b in USS is 1.5, 1.7 and 1.6, so **DETECTED** this time (bar 1.41). Round 1 was 1.3 / 0.6 / 1.3. It is still marginal, and S2c should not treat it as a stable gate.

**Playing, MiB (PSS / USS):**

| Run | pin | s2b |
|---|---|---|
| 1 | 163.6 / 141.2 | 164.7 / 142.3 |
| 2 | 163.8 / 141.3 | 166.2 / 143.7 |

- Covers while playing add +1.1 and +2.4 MiB (round 1: +3.4).
- The whole app plays at **164.7–166.2 MiB PSS** with 50 covers visible.
- The screenshot was regenerated at `target/screens/s2b-covers-playing.png`: 50 covers, Playing at 0:42.

## Round 3: review fixes (4dcb35d)
- **Binaries:** release `bramley-s2b` sha256 prefix 7e77a8b8441d72cd; dev `bramley-s2b-dev` 8bca5a944fc66da4.
- **Verdicts:** `docs/results/s2b-covers/r3/`.
- **First session: VOID.** The reference read 88.3 and 89.0 against 83.3 (drift 5.35 > 5). Load average was about 10 from other work on the box. Its evidence is kept in the worktree's `scratch/s2b/session5-void/`.
- **Re-run: VALID.** The reference read 87.7 both times, 4.4 from 83.3. Its USS was 60.8 (recorded 60.9), so the drift sits in shared pages.

| Check | Result |
|---|---|
| Guard self-test | killed 0.26 s after its last clean poll; the default sink was unchanged; plays now wait for at least one clean poll |
| Release `--screenshot` | exit 1, "unknown argument --screenshot", no PNG |
| The same check on the dev build (control) | no "unknown argument": the check can fail |
| Signed out, developer token present | SignInNeeded, 0 requests, 0 textures, alive at the end |
| Expired token (dev build, local server answering 401) | the server saw only `/v1/me/storefront`; SignInNeeded; 0 textures; alive at the end |
| Signed in | 52 requests (storefront, chart, 50 covers), 50 uploaded, 0 failed, 0 in flight |

**Memory, MiB (PSS / USS):**

| Run | ref | pin | s2b |
|---|---|---|---|
| idle 1 | 87.7 / 60.8 | 67.1 / 40.2 | 71.7 / 44.8 |
| idle 2 | 87.7 / 60.8 | 67.4 / 40.5 | 71.7 / 44.8 |
| playing | | 167.8 / 140.9 | 169.5 / 142.6 |

- **Metric rename:** `decodedBytesHeld` (rounds 1 and 2) is now `decodedInFlight`. gate-sabotage's did-it-run check is `decodedBytesKept`, which is derived from what it really keeps (unit-tested; not run live this round).
- **Per cover:** 4.45 / 50 = **0.09 MiB** in PSS and in USS.
- **Covers while playing:** +1.7 MiB.
- **The whole app while playing:** 169.5 PSS, inflated by this session's drift; 142.6 USS.
- **Screenshot:** taken with the dev build of the same commit (`--screenshot` is dev-only now): 50 covers, Playing at 0:41.

**Token trace (4dcb35d):**
- **Setup:** a signed-in release run under `RUST_LOG=trace`.
- **It ran:** one `/v1/me/storefront` request, logged by ureq with `"2 HEADERS ARE REDACTED"` (Authorization and Music-User-Token). The chart request shows 1.
- **Result:** 0 hits for either token in the log and status files, 0 JWT-shaped strings and 0 `music-user-token` mentions. The log stays out of the repo because it holds mzstatic URLs.

**Apple's statuses for `/v1/me/storefront` (curl, 2026-10-10):**
- a corrupted developer token gives **401**;
- a garbage user token gives **403**;
- valid tokens give 200.

So mapping 401 to SignInNeeded, as specified for this round, conflicts with S4b's "a developer-token 401 never reads as sign in". Flagged; not changed.

## Known limits
- **`coversRequested` resets with the grid.** After a rejection, the status reads 0. The expired control's server log is the real count: 1 storefront request.
- **Not pre-registered:** the round-2 and round-3 controls weren't written into the PREREG before they ran. Their pass conditions came from the coordinator's review messages. These are signed-out, release refusing `--screenshot`, the expired token, and the guard's exposure metric.
- **No retry:** a storefront or chart network failure is never retried. Acceptance F10 needs automatic recovery.
- **Zoom:** each zoom step re-runs the storefront check plus 51 requests, and the grid blanks while it reloads.
- **Panic catch:** a decoder panic counts as a failure, but nothing in the tests can trigger one, so that path is untested.
- **Re-sign-in:** if sign-in comes back, the grid refetches on its own. Nothing can turn it back on until S3b.
- **Machine verdicts:** the verdict files hold data, and PASS/VOID was judged by hand. S2c and S13 own computed verdicts. Don't reuse this harness as a gate.
- **Public screenshot:** there is no synthetic-cover screenshot for a public PR yet.

## Re-run
```
cargo build --release -p bramley --features dev && cp target/release/bramley target/reference/bramley-s2b-dev
cargo build --release -p bramley && cp target/release/bramley target/reference/bramley-s2b
# target/reference also needs bramley-main-4961e27 (main) and bramley-pin-e32305a (b824098)
sg render -c "python3 -I scripts/app/covers-session.py --out target/app-runs/s2b-covers $(cat scripts/app/covers-runs.txt)"
```
Round 1 ran the same driver from the worktree's `scratch/` before it moved into the repo. Round 2 also used `bramley-s2b-r1` and `bramley-s2b-sabotage`.
