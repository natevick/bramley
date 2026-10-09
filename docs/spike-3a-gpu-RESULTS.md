# Spike 3a-GPU results (2026-10-09)

**Verdict: MARGINAL.** The fastframe build measured 85.4 MiB PSS median against the ≤ 70 pass bar. The fail line is 100.

| Build | PSS median | RSS | USS | GL renderer |
|---|---|---|---|---|
| bare eframe glow | 59.1 | 91.4 | 31.8 | AMD Radeon RX 5700 XT (radeonsi, navi10, ACO) |
| fastframe (fonts, theme, icons, text, 1,000-row list) | 85.4 | 117.4 | 58.6 | same |

**Did-it-run checks:**
- The renderer is the real AMD GPU, not llvmpipe.
- Weston's log shows "Using GL renderer" on radeonsi.
- frames > 0. Only 1 frame was presented. Weston headless sends no frame callbacks without damage, and the idle egui app requests no repaint. Under Xvfb the app kept repainting, which is why that run logged 100+ frames.

**What it means:** software rendering inflated the Xvfb number by about 22 MiB. fastframe adds about 26 MiB over bare eframe; fonts and emoji are the likely bulk, not yet broken down. UI plus engine comes to about 183 MiB, under 200. See `docs/budget.md`.

Re-run: `sg render -c 'spikes/03-native-cdm-and-ui/run-3a-gpu.sh bare fastframe'`
