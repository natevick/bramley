# Memory budget (pre-registered 2026-10-09)

The target is ≤ 200 MiB PSS for the whole app tree while playing, measured with the spike-1 method (sum of `smaps_rollup` Pss over every process in the tree, median over 60 s of steady playback). RSS and USS are reported alongside.

| Slice | Budget (MiB PSS) | Measured |
|---|---|---|
| egui/fastframe UI (window open, idle) | ≤ 70 | **108.6–109.6** under Xvfb + Mesa llvmpipe (FAIL as measured); ~41 of it is llvmpipe libraries (libLLVM 31.8, libgallium 9.5). Bare eframe: 82.0–85.8. Real-GPU number unmeasured (spike 3a) |
| JS runtime + musickit.js loaded and configured | ≤ 40 | **8.5** QuickJS-NG (V8: 33.2), idle after configure; playback not yet measured (spike 4) |
| Widevine CDM + demux + AAC decode + audio output | ≤ 90 | **86.0–86.6** PASS (spike 3b); ~81 of it is the CDM itself (57 MiB of its .bss resident + 20 MiB of its code) |
| **Total** | **≤ 200** | |

These are targets, not a commitment. If a slice is measured over its target, the split may be rebalanced only if the measured total still fits; the 200 total does not move. For reference, the Chrome sidecar measured 398 MiB (spike 1, FAIL).
