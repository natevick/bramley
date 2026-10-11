# S2c whole-app memory gate: pre-registration (2026-10-10)

S2c is the first slice of S13's acceptance harness. `scripts/acceptance.sh <run> [--llvmpipe]` implements acceptance.md's R1 and R2 with computed verdicts. These conditions are fixed before the first counted run. Thresholds come from acceptance.md and do not move here.

## Changes from the plan (coordinator's brief, 2026-10-10)
- **Files:** `scripts/acceptance.sh` and `scripts/acceptance/gate.py` (verdict self-test: `scripts/acceptance/test_gate.py`), not `measure-app.sh` and `verdict.py`. This PREREG replaces `app-shell-memory-PREREG.md`.
- **Sabotage A is the ballast, not RGBA retention.** S2b showed the retention rise (1.76 MiB) is not reliably measurable. The ballast is acceptance.md's control. `gate-sabotage` still keeps its RGBA copies, which add about 1.8 MiB on top of the ballast.
- **Out of scope here:** plan sabotage B (empty token dir) and C (unreachable art host), and acceptance.md's "No user token" control. They land with later harness slices.

## Binaries
Built by `acceptance.sh` from HEAD in one sitting. Each is copied to `target/acceptance/<run>/bin/` straight after its build, and only the copies are launched. Their sha256 prefixes go into the verdict.
- `bramley`: release, shipped features. `check-shipped-features.sh` must exit 0 on it, or nothing runs and the gate is FAIL.
- `bramley-ballast`: release, `--features gate-sabotage`. `check-shipped-features.sh` must exit exactly 1 and list `app/gate-sabotage`.
- `bramley-dev`: release, `--features dev`, for the screenshot only.
- Reference: `target/reference/bramley-main-4961e27`, sha256 prefix `1a72eb4b2880ac5a`, recorded idle PSS 83.3 MiB.

## Session
One weston headless `--renderer=gl` session under `sg render`, with a PipeWire null sink, both from `covers-session.py`'s `session()`. Each run executes in order: reference, shipped, ballast, screenshot. Before every launch, `bramley now` must exit 3; otherwise the harness refuses to launch.

## Method
- **Memory:** the sum of `smaps_rollup` over the app and its children. Each run records PSS (the verdict), USS and RSS medians.
- **App state:** read only through `bramley now`, once per sample. The shipped binary is launched with no arguments and driven only through the CLI (`now`, `play 6781024437`).
- **Reference:** first frame, then a 12 s settle, then 20 samples at 1 s. This is the method that recorded 83.3.
- **R2 (browsing, never played):**
  - Wait for the first frame and `texturesUploaded` ≥ 50, then a 5 s settle.
  - Take 20 samples at 1 s.
  - PASS if the median ≤ **100 MiB**.
- **Audio before play:**
  - The null sink exists by exact `node.name`.
  - The default sink is not the null sink.
  - The guard has made at least one clean poll and has not fired.
  - Any miss refuses the play, and R1 then has no samples (FAIL).
- **Play:** run `bramley play 6781024437`, then wait up to 90 s for Playing, no banner, position > 15 s and `applied ≥ seq`.
- **R1:**
  - Take 60 samples at 1 s.
  - PASS if the median ≤ **200 MiB**.
- **Guard:** it runs from launch to exit and SIGKILLs the app if any of its streams links to anything but the null sink.

## Verdict rules (computed by `gate.py verdict()`, a pure function of the evidence directory)
Applied per check, in this order:

1. **FAIL, if any evidence file is missing:** `build.json`, `session.verdict.json`, `reference.json` (with samples), or the check's own `shipped.r1.json` / `shipped.r2.json`. Missing evidence is FAIL even when the session is otherwise VOID.
2. **VOID, if any of these hold:**
   - a reference or app sample's `glRenderer` lacks `radeonsi` or contains `llvmpipe`;
   - the reference sha differs from the recorded one;
   - the reference PSS median is more than 5 MiB from 83.3;
   - the default sink changed;
   - the null sink was left behind;
   - the guard killed the app;
   - R1 only: the position advanced ≥ 50 s but no stream was linked to the null sink.
3. **FAIL, if any of these hold:**
   - the shipped binary failed `check-shipped-features.sh`;
   - fewer samples than required;
   - any sample with fewer than 50 covers;
   - not exactly 1 process;
   - the median is over its bar;
   - R2 only: the engine started;
   - R1 only:
     - playback never started;
     - the position advanced < 50 s across the window;
     - any sample not Playing, or with a banner.
4. Otherwise **PASS.**

Every reason is recorded, not just the first. **gate** = PASS only if R1 and R2 PASS and the ballast control holds. It is VOID if R1 or R2 is VOID, otherwise FAIL.

## Controls
- **Ballast:**
  - Sized from this run's `shipped.r1.json`: `ceil(215 − shipped R1 median)` MiB, passed as `BRAMLEY_GATE_BALLAST_MIB`. The binary touches every page at startup.
  - The control **holds** only if all of the following are true:
    1. its R1 verdict is FAIL and the only reason is the PSS threshold, so it was playing, had covers, ran on radeonsi and was valid;
    2. its median lands within 215 ± 5;
    3. the recorded MiB equals the size recomputed from the shipped evidence;
    4. its binary fails `check-shipped-features.sh` with exit 1 naming `app/gate-sabotage`.
  - With no shipped R1 median, there is no ballast run and the control does not hold.
- **llvmpipe forced** (`--llvmpipe`):
  - `LIBGL_ALWAYS_SOFTWARE=1` is set for the apps under test only. weston and the reference keep the GPU, so a VOID can't come from reference drift.
  - The control holds only if R1 and R2 are both VOID with a renderer reason, and the shipped samples' `glRenderer` really contains llvmpipe.
- **Guard:** after `covers-session.py`'s setup moved into `session()`, its `guardtest` mode is re-run once and must still kill the app.
- **Verdict self-test:** `python3 -I scripts/acceptance/test_gate.py` covers four cases: an empty directory is FAIL, llvmpipe is VOID, missing R1 evidence in a VOID session is still FAIL, and a ballast landing out of band does not hold.

## Pass condition for S2c
1. Two consecutive normal runs, each with R1 PASS, R2 PASS and the ballast control holding.
2. One `--llvmpipe` run whose control holds.

Runs from a dirty tree don't count. A VOID run is re-run and reported, never dropped.

## Reported, no verdict
- USS and RSS next to PSS.
- `mem_info_vram_used` before and after R1. Named false positive: textures in VRAM don't show in PSS.
- The screenshot, taken with the dev build of the same commit at about 45 s. It goes to `target/screens/`, never into the repo.

Committed verdict files hold numbers and reasons only: no logs, art URLs or tokens.

## Amendment 1 (2026-10-10, S2c review round 1, before any further counted run)
Procedure only. The 200/100 bars, the 5 MiB drift bar, the recorded 83.3 and 215 ± 5 are unchanged. Runs before this amendment (run1, run2, llvmpipe1, norm-run1) are scored under the original rules and are not re-scored.

- **Zones:** each check's medians, recomputed from the samples, fall into one zone.
  1. **USS > bar:** FAIL.
  2. **RSS ≤ bar:** PASS-eligible. Drift is reported but does not void.
  3. **Otherwise:** judged on PSS. VOID if the reference drifted more than 5 MiB.
- **Reference:** it is measured before the app runs and again after the ballast, with the same method. The worse drift counts. Its sha, pid and renderer checks apply to both readings.
- **Precedence:**
  - Any missing evidence is FAIL and beats VOID. The evidence files are `build.json`, `session.verdict.json`, `weston.log`, both reference files, the check's own file and `<run>.guard.json`.
  - A tree that is dirty, or whose dirty state is unknown, is FAIL.
- **New VOIDs:**
  - weston's own `GL renderer:` log line is missing, or isn't radeonsi;
  - `wpctl` returns nothing at the start, at the end or before play;
  - the default sink changed, even if the harness restored it;
  - a guard kill, now on R1 **and** R2;
  - in the R1 window, fewer than 30 guard polls, or any poll where our stream isn't linked to the null sink only (each poll is timestamped).
- **New FAILs:**
  - a `bramley now` snapshot whose `pid` isn't the sampled app's;
  - the ballast VOID alone makes the gate VOID, not FAIL.
- **Audio:**
  - pw-dump times out after 1 s, and a timeout kills the app.
  - Once the app first reports Playing with position > 0, the guard must have seen our stream linked to the null sink within 2 s. Otherwise it kills the app (VOID).
  - After any guard kill, the remaining playing phases are skipped.
  - If the default sink changed, the session restores it and records the change.
- **Ballast:**
  - Sized `ceil(215 − shipped R1 USS median)` MiB, so it lands in zone 1.
  - It holds only if all of the following are true:
    1. R1 is FAIL with only the USS reason;
    2. the USS median is within 215 ± 5;
    3. the measured USS rise over shipped is within 3 MiB of the requested size;
    4. the feature check fails as before.
  - `sizedFromThisRun` was a self-comparison and is dropped.
- **Pre-flight:** `acceptance.sh` refuses to start (exit 2) and prints what it found when it sees any of the following:
  - another weston;
  - another bramley;
  - any process outside the standing set that maps libgallium, radeonsi or libLLVM. The standing set is gnome-shell, Xvfb and grok-bot, all running since 2026-09-16, before run1, run2 and the 83.3 recording.
- **Build order:** ballast, then dev, then shipped, so `target/release/bramley` ends as the shipped build.
- **Pass condition:** unchanged. Two consecutive counted runs PASS with the ballast holding, plus one `--llvmpipe` run whose control holds.
