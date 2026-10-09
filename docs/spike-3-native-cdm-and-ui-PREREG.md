# Spike 3: UI baseline and native Widevine CDM host (pre-registered 2026-10-09)

The bars cannot move after data arrives. Measurement method is the same as spike 1: sum `smaps_rollup` Pss/Rss/USS over the process tree, sampled every 2 s for 60 s after the warm-up, and report the median and max.

## 3a: UI baseline
- **Build:**
  - A bare eframe 0.36 glow window.
  - A second build of the same window adding the fastframe crates a real app would use (fonts, theme, icons), plus a virtualised 1,000-row list.
- **Run:** under `xvfb-run` with Mesa llvmpipe, which inflates GPU-side memory. Record the GL renderer string.
- **Pass:** the fastframe build is ≤ 70 MiB PSS idle, with the window shown and 5 s of warm-up first. Marginal: 70–100. Fail: above 100.
- **Did-it-run:** the GL renderer string was logged, at least one frame was presented (log the frame count), and the tree has ≥ 1 process.

## 3b: native CDM host
- **What it is:**
  - A Rust binary that loads the installed Chrome Widevine CDM (`/opt/google/chrome/WidevineCdm/_platform_specific/linux_x64/libwidevinecdm.so`) through its official C interface. It never extracts keys.
  - It plays Google's public Widevine demo audio (the Shaka assets used in spike 1) through `cwip-shaka-proxy.appspot.com/no_auth`.
  - Pipeline: demux fMP4, decrypt CENC samples through the CDM, decode AAC, output to PipeWire.
- **Before writing the host:** check which `CreateCdmInstance` interface versions the library actually exports. The manifest says interface 10, host 10–11.
- **Write it against Chromium's BSD-licensed `content_decryption_module.h`.** Do NOT copy code from Kopuz or cdm-host: they are EUPL-1.2.
- **Pass:** median PSS ≤ 90 MiB over 60 s of steady playback, for the whole process including the CDM.
- **Did-it-run (all required, or the run is void):**
  1. A key status of usable from the CDM.
  2. More than 30 s of audio decoded from decrypted samples. Log the sample count; at least one sample must come from an encrypted range.
  3. A PipeWire output stream node present while playing. Use a temporary null sink, as spike 1 did.
  4. A sabotage run with a broken license URL reads VOID.
- **Named false positives:**
  - Clear lead-in samples: Sintel's first ~12 s are unencrypted. Check 2 must count decrypted samples, not just decoded ones.
  - PSS deflated by Chrome processes sharing the CDM library. Report USS as well.
  - Audio decoded but never sent to the output. Check 3 guards this.
