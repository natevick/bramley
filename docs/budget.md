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
