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

**App skeleton, S2a (2026-10-10):** `bramley` with the engine started lazily: **82.3** PSS idle (release, never played) and **182.5** playing (dev build, no covers). See `s2a-skeleton-RESULTS.md`. S2c owns the gate.

**Whole-app gate, S2c (2026-10-10):** release `bramley` with 50 covers visible, in two consecutive `scripts/acceptance.sh` runs under the zone rule (round 2), both PASS in zone 2. R1 playing: **165.1 / 164.2** MiB PSS (USS 142.8 / 141.8, RSS 193.2 / 192.3). R2 browsing, never played: **66.9 / 67.0**. The reference read 83.5–83.7 before and after. See `s2c-gate-RESULTS.md`.

**Layout shell, S6 (2026-10-11):** sidebar, routed content, player bar and queue panel; no new crates. What was measured is the gate's own scenario only: the default route (Home, with 50 covers), the queue panel closed, and the dark theme, because weston has no settings portal. Route switching, an open queue panel and the light theme were not measured. Gate `s6-2` (5d65f39) PASS in zone 3: R1 playing **170.9** PSS / 144.2 USS / 203.6 RSS, R2 browsing **73.9** / 47.1 / 106.5, reference 90.0 / 90.0. A clean `main` (1a6d82a) gated earlier the same morning read R1 170.7 / 143.9 / 203.5 and R2 73.2 / 46.3 / 105.9, with the reference at 88.9 / 89.7. The deltas (+0.2 and +0.7 PSS) are smaller than the reference's own movement.

**Search, S7 (2026-10-11):** no new crates. Gate `s7-4` (541173e) PASS in zone 3: R1 **172.4** PSS / 145.5 USS / 205.0 RSS, R2 **74.7** / 47.8 / 107.2. The earlier session's clean `main` (f35def2) read R1 172.0 and R2 74.6, a difference within the reference's +1.7 drift. The gate never opens Search, so search art (40 pt, at most 30 textures, about 6 KiB each) is unmeasured. See `s7-search-RESULTS.md`.

**Library lists, S9 (2026-10-11):** gate `s9-3` (e1f5adb) PASS in zone 3: R1 playing **171.0** PSS / 142.7 USS / 205.2 RSS, R2 browsing **73.5** / 45.1 / 107.6, reference 89.9 / 89.3. Same-day main (f35def2, another session) read R1 172.8 and R2 74.7 with the reference at 88.7. On the lead over the reference that is −2.7 and −2.1 MiB, within session noise. Report-only, dev build: Songs route idle 74.3 against a Home control of 73.7. Scrolling all 4,481 songs to the end added **+2.6** PSS (bar ≤ 10). Playing with a library playlist open read 171.5. Library art is an LRU of 96 textures, which reached its cap and held. See `s9-library-RESULTS.md`. After the review and the rebase onto S7, gate `s9-4` (fd854b8) also PASSed in zone 3: R1 **174.0** / 147.1 / 206.6 and R2 **75.5** / 48.6 / 108.1.

**Album and playlist detail, S8 (2026-10-11):** no new crates. Gate `s8-2` (1921d0e) PASS in zone 3: R1 playing **171.6** PSS / 144.7 USS / 204.2 RSS, R2 browsing **74.3** / 47.4 / 106.9, reference 88.4 / 88.7. Against S9's `s9-4`, on the lead over the reference, that is −1.2 and 0.0 MiB, within session noise. The gate never opens a detail view; its art is one 160 pt texture, held only while the view is open. See `s8-detail-RESULTS.md`.

**S8 review round (2026-10-11):** gate `s8-3` (e956bff) PASS in zone 3: R1 **173.1** / 146.2 / 205.7, R2 **75.5** / 48.6 / 108.0, reference 90.2 / 90.1. Against `s9-4`, on the lead over the reference, that is −1.3 and −0.4 MiB, which is noise.
After the detach-count fix, gate `s8-4` (eef8d07) PASSed in zone 3: R1 **172.3** / 145.5 / 204.9, R2 **75.4** / 48.5 / 108.0, reference 89.7 / 89.6. Against `s9-4` on the lead over the reference: −1.6 and 0.0 MiB.

**v1 acceptance (2026-10-13):** main 07be8d1, which adds the engine's in-memory HTTP cache (S14b; at most 2 MiB and 256 entries) and pooled connections. Counted runs v1-2 and v1-3 PASS in zone 3: R1 playing **174.0 / 174.3** PSS (USS 147.2 / 147.4, RSS 206.7 / 207.1), R2 browsing **75.6 / 76.1** (USS 48.7 / 49.1), reference drift +1.1 / +0.9. The cache was not measured on its own; its cap is inside these R1 readings, which sit within 1 MiB of S9's `s9-4` (174.0). See `v1-acceptance-RESULTS.md`.

**Home grid art size, S43 (2026-10-21), estimate only, not measured:** Home covers now scale to fill full rows, up to 200 pt a side, so they are decoded at `min(200 pt × pixels_per_point, 256 px)` instead of 96 pt × pixels_per_point. The size depends on the scale factor alone, never on the window width, so a resize does not refetch. The 256 px cap is hard. Texture cost is 50 × px² × 4 bytes: 96 px is 1.76 MiB, 144 px 3.96, 192 px 7.03, 200 px 7.63, 256 px 12.5. The change adds 5.5 MiB at scale 2, 5.9 at 1, 8.5 at 1.5, and at most about 9.6 at 1.28, where the cap first bites. The R1 reading to add it to is v1 acceptance's 174.3 PSS, so the worst case is about 184 against the 200 bar if every texture byte counts as host memory; on the GPU path part of it sits in VRAM and counts less. Decoded pixels still drop after upload. The acceptance run is the real measurement. `bramley warm` sizes its Home art with the same function, so it fills the cache entries Home reads.

**Home live refresh, review fix (estimate only, not measured):** while a live refresh runs, `fresh` and `textures` both hold covers until the swap, so Home art can briefly double: up to about +12.5 MiB at 256 px (50 × 256² × 4 bytes), on top of the worst case above.

## macOS (2026-10-22, S47b, Nate: "mac budget should be what you suggested")

The Linux budget above is PSS on Linux and does not carry over: macOS has no PSS, and the biggest macOS cost is the window surface, which the 200 MiB split never had a line for. macOS gets its own budget, measured on the MacBook Air with `footprint -p`:

- **At most 100 MB `phys_footprint` at the default window (848 x 1062 pt, 2x), idle on Home, with at most 2 window-sized IOSurface buffers.**

Why: S47 (`~/bramley-spike/s47` on the Air) measured the window surface at the default size. glow (OpenGL) holds 5 window-sized IOSurface buffers, 130 MB phys. wgpu on Metal with the egui fork's `macos-window-resize-jitter-fix` holds 3, 91 MB. wgpu without that feature holds 2, 78 MB. The Mac build uses wgpu without the fix (`crates/app/Cargo.toml`, `crates/app/src/main.rs`): present mode Fifo and a maximum frame latency of 1, the lowest `wgpu-hal`'s Metal backend takes. Linux stays on glow. `scripts/macos/check-renderer.sh` fails if the Linux tree gains wgpu or the macOS tree gains glow, the jitter fix, or a wgpu backend other than Metal.

Window surfaces scale with window pixels, not with the default size. At a 5K display at full screen the surface is about 59 MB per buffer by extrapolation from the measured 13.3 MB at 1696 x 2060 px, so 2 buffers is about 118 MB there. That is extrapolation, not a measurement; the 100 MB bar applies at the default window only.

The jitter fix exists to keep live resizing smooth. Dropping it is the cost of the 2-buffer surface, and resize smoothness is checked by `docs/s47b-PREREG.md` M3.

Measured (S47b, `docs/s47b-RESULTS.md`, Developer ID build, default 848 x 1062 pt window, Home with 50 covers, idle): the counted sample read **102.3 MiB** with 2 window-sized buffers, 2.3 MiB over the bar. Three report-only replicates read 96.3, 94.2 and 95.3 (median 95.3). Two minutes of muted playback read 98.7 to 98.8, flat. The bar is not met on the pre-registered sample; Nate decides whether the replicates are the number.

