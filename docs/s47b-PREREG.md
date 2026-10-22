# S47b: macOS wgpu/Metal renderer, pre-registration (2026-10-22)

Change under test: commit 98b8d5e (`natevick/mac-metal`). macOS draws with wgpu on Metal (Fifo, maximum frame latency 1, no `macos-window-resize-jitter-fix`); Linux keeps glow. This run CONFIRMS S47's exploratory variant C2 (78 MB, 2 IOSurface buffers), which was added after S47's data was seen.

Source: `git archive 98b8d5e`, built on the Air with `. ~/bramley-spike/macnative-env.sh` and `CARGO_TARGET_DIR=~/bramley-spike/target-s47`. Work dir: `~/bramley-spike/s47b/`. Before bundling: `cargo clippy -p bramley --all-targets -- -D warnings` and `cargo test -p bramley --no-run` must pass on the Air, because the macOS `cfg` branches cannot compile on Linux.

## Budget under test

At most 100 MB `phys_footprint` at the default window (848 x 1062 pt, 2x), idle on Home, with at most 2 window-sized IOSurface buffers (`CAMetalLayer Display Drawable` regions of 1696 x 2060 px, 13.6 MB each).

## Known confound: ad-hoc builds are signed out

S47's ad-hoc runs read `banner: true`, `state: Stopped`, route Home, with no covers (MusicKit `notDetermined`). So M1 measures the window surface plus an empty Home. It cannot decide the budget on its own. The budget verdict is M4-idle, taken on the Developer ID build with Home's covers uploaded. If M4-idle is over 100 MB, that is reported as the budget NOT met, whatever M1 read.

## M1: window surface, default window, ad-hoc, frontmost

3 launches of the new build, 45 s each, frontmost via `open` (no `-g`), `caffeinate -d -u` held. Harness: S47's `tools/measure.sh` (pid by exact binary path, other-bramley-process check, `footprint --json`, `vmmap -v` regions, `winfo` window size, `bramley now` at t40 and t45).

PASS only if ALL hold, on the median of the 3 launches unless stated:
1. `phys_footprint` <= 100 MB.
2. Window-sized IOSurface regions (size equal to the measured window w x h x scale^2 x 4 B) <= 2.
3. Every launch: `framesPainted` rises between t40 and t45, and `glRenderer` starts with `Metal`.

Expected, from S47 C2: about 74 to 78 MB and 2 regions. Anything above 90 MB or 3 regions is a miss to explain, not round down.

Void a launch if: the display is asleep or locked, the window is not frontmost, the window is not 848 x 1062 (record it), another bramley process (the warm agent runs every 300 s) overlaps, or the pid measured is not the bundle under test.

Harness can fail (run before M1 counts): one launch of S47's glow bundle `~/bramley-spike/s47/A/Bramley.app` under the same script. The harness must report it as FAIL on criteria 1 and 2 (S47: about 130 MB, 5 regions). If it does not, M1's verdict is invalid.

## M2: maximized window

Not attempted unless it can be done with no TCC prompt. The app has no window control and no CLI command resizes it; System Events, AXUIElement and synthetic mouse events all need Accessibility, which raises a prompt on the Air's screen. Expected result: SKIPPED, with the surface size taken by extrapolation from window pixels only. Skipped is not a pass.

## M3: live-resize smoothness, the risk of dropping the jitter fix

Not drivable over ssh. A programmatic resize (`ViewportCommand`, `setFrame`) does not put the NSWindow into live resize, which is the case the jitter fix exists for, so a scripted resize that looked clean would be a false clean. Recorded as NEEDS A HUMAN CHECK, not run, not a pass.

What jitter would look like (write-down before any test):
- window content lagging behind the moving frame edge, or trailing it by a visible gap;
- stretched or squashed content (the previous frame scaled to the new size);
- black, white or transparent bands at the growing edge;
- UI anchored to the far edge (player bar, scrollbar) wobbling or flickering while the window resizes.

Human protocol: bundle at `~/bramley-spike/s47b/new/Bramley.app` against `~/bramley-spike/s47/C/Bramley.app` (with the jitter fix); drag the bottom-right corner slowly then fast for 10 s each, and the left edge once. The new build is acceptable if it shows none of the four signs that C does not. If it shows any of them, the remedy is wgpu with the jitter fix (3 buffers, 91 MB, over the 2-buffer budget), which is Nate's call.

## M4: Developer ID build, probe, idle on Home, muted playback

Signed with "Developer ID Application: Nathan Vick (C9E4T9WRC5)" and `~/Downloads/Bramley_Developer_ID.provisionprofile`, in the GUI session via `open <script>.command` (the identity is in the login keychain, locked to ssh). Not notarized. `/Applications/Bramley.app` and the warm agent are not touched.

Checks, in order:
1. `musickit-probe` (status, no `--request`): `authorization` is `authorized` and the token lengths are above 0. If it is `notDetermined`, a consent prompt would be needed: stop and report BLOCKED rather than click anything.
2. M4-idle: launch frontmost, wait for route Home, state Stopped and `texturesUploaded` >= 30 (up to 120 s), then 45 s more. Sample `footprint` and the IOSurface regions. This is the budget verdict: <= 100 MB and <= 2 window-sized regions. The counted sample is the one taken before any playback.
3. Mute: capture `osascript -e 'get volume settings'` (all four fields) to a file, require the default output to be the built-in speakers, set `output muted true`, and verify `output muted:true`. If the device cannot mute, abort; do not play.
4. Playback: `bramley play <id>` with an id from `~/bramley-spike/mem-play/ids.tsv`, then 120 s sampled every 15 s. Output is filtered to state, position, duration, queue position, route, banner and counters; no titles, names or library contents are printed or saved.

Playback passes if ALL hold:
- every sample is valid, meaning state `Playing` and a position that moved (or the queue position changed), and a parsed `phys_footprint`; an invalid sample is void, not clean;
- footprint is flat: max minus min across the samples <= 10 MB and last minus first <= 5 MB;
- window-sized IOSurface regions stay <= 2 in every sample.

The playing footprint is reported against idle, not against 100 MB; the budget's idle condition is check 2.

Restore, always (trap): pause, quit the app by pid, restore the saved output volume, input volume, alert volume and mute state exactly, and require `get volume settings` to equal the captured text. Any difference is reported as a failure of the run.

## Named false positives

- Sleeping, locked or occluded display dropping surfaces: IOSurface shrinks without the change doing anything.
- Different window size: each region's pixel size must equal the measured window.
- Wrong process: `open` handing off to a running instance of the same bundle id, or measuring the warm agent.
- The new build still being glow: it must show no `OpenGL GLSL` category and `glRenderer` must say Metal.
- A smaller footprint bought with a repaint stall: criterion 3 of M1 and the per-sample position check of M4.
- The signed-out confound above: an ad-hoc number standing in for the idle-on-Home number.

## Amendment log

- 2026-10-22 12:58, AFTER the first M1 round (4 launches: 1 glow control, 3 new) and before the signed runs. The window was 360 x 1062 pt, not 848 x 1062. macOS places a new window in the free tile of the desktop, and Nate's screen has four Terminal/Ghostty tiles (S47's 848 pt was the gap at that time), so "default window" is whatever the OS gives and I do not control it. Data already seen: new build about 58 MiB measured with 2 regions of 720 x 2060 px; glow control 87.5 MiB with 5 regions.
  - A smaller window can only understate surface memory, so the measured number cannot show the budget is met. The verdict number is normalized to the 848 x 1062 pt window: `normalized = measured + N x (1696 x 2060 x 4 B - bytes of one measured window-sized region)`, N the window-sized region count. Both numbers are reported. The control's normalized value must land near S47's glow figure (about 125 to 130) for the normalization to be trusted.
  - Window-sized region means an IOSurface whose pixel width is 2 x the window's width and whose height is within 60 pt of the window height (the title bar is not in the surface). M1 criterion 1 reads the normalized median; criteria 2 and 3 stand as written; the "window is 848 x 1062" void rule is replaced by: record the size, void only if it differs between launches of one set.
  - The same normalization applies to M4-idle and M4 playback. Any window of 848 x 1062 pt or larger is read as measured.
- 2026-10-22 13:10, AFTER all M4 data. Three harness notes, none of which changes a threshold:
  - The output mute and its verification run before the idle sample, not after it, so nothing can sound at launch. Playback still starts only after the idle sample.
  - Two M4 attempts aborted before sampling ("no pid"): the harness's pid match assumed a 4-digit pid (`ps` pads 5-digit pids differently). The leftover signed-app processes were stopped by pid, and the volume settings were restored exactly both times. Fixed in `tools/m4.sh` and `tools/measure.sh`.
  - After seeing M4-idle read 102.3 MiB, three more signed idle-on-Home launches were taken with `tools/measure.sh` (45 s, frontmost). They are REPORT-ONLY. The counted M4-idle sample stays the one taken before playback.

