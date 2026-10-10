# Memory budget (pre-registered 2026-10-09)

The target is ≤ 200 MiB PSS for the whole app tree while playing, measured with the spike-1 method (sum of `smaps_rollup` Pss over every process in the tree, median over 60 s of steady playback). RSS and USS are reported alongside.

| Slice | Budget (MiB PSS) | Measured |
|---|---|---|
| egui/fastframe UI (window open, idle) | ≤ 70 | **85.4 on the real GPU (AMD RX 5700 XT, radeonsi; spike 3a-GPU, MARGINAL)**; bare eframe 59.1. Under Xvfb/llvmpipe it was 107–110 |
| JS runtime + musickit.js loaded and configured | ≤ 40 | **8.5** QuickJS-NG (V8: 33.2), idle after configure; playback not yet measured (spike 4) |
| Widevine CDM + demux + AAC decode + audio output | ≤ 90 | **86.0–86.6** PASS (spike 3b); ~81 of it is the CDM itself (57 MiB of its .bss resident + 20 MiB of its code) |
| **Total** | **≤ 200** | |

These are targets, not a commitment. If a slice is measured over its target, the split may be rebalanced only if the measured total still fits; the 200 total does not move. For reference, the Chrome sidecar measured 398 MiB (spike 1, FAIL).

Engine crate (2026-10-09): `crates/engine` playing Angel One through hls.js, the native media/MSE/EME layer, the Widevine CDM and PulseAudio in one process measured 97.6 and 97.7 MiB PSS median (the JS runtime slice plus the CDM slice together; musickit.js not loaded). See `engine-crate-playback-RESULTS.md`.


**Rebalance, 2026-10-09:** UI 85.4 plus the playback engine in one process (QuickJS + CDM + pipeline) at 97.2 comes to about 183 MiB summed PSS, which still fits under 200. The UI slice overran its target and the engine came in under its 130 (40 + 90). The total has not moved. Not yet measured: real musickit.js plus Apple's hls.js during playback, and the UI with album art loaded.

**App skeleton, S2a (2026-10-10):** `bramley` with the engine started lazily: **81.6** PSS idle (release, never played) and **180.0** playing (dev build, no covers). See `s2a-skeleton-RESULTS.md`. S2c owns the gate.
