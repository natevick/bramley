# Memory budget (pre-registered 2026-10-09)

The target is ≤ 200 MiB PSS for the whole app tree while playing, measured with the spike-1 method (sum of `smaps_rollup` Pss over every process in the tree, median over 60 s of steady playback). RSS and USS are reported alongside.

| Slice | Budget (MiB PSS) | Measured |
|---|---|---|
| egui/fastframe UI (window open, idle) | ≤ 70 | pending: spike 3 |
| JS runtime + musickit.js loaded and configured | ≤ 40 | pending: spike 4 |
| Widevine CDM + demux + AAC decode + audio output | ≤ 90 | pending: spike 3 |
| **Total** | **≤ 200** | |

These are targets, not a commitment. If a slice is measured over its target, the split may be rebalanced only if the measured total still fits; the 200 total does not move. For reference, the Chrome sidecar measured 398 MiB (spike 1, FAIL).
