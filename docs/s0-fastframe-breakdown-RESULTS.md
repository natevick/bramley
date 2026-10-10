# S0 results: fastframe's +26 MiB (2026-10-10)

**Verdict:** fallback fonts are 23.4 of fastframe's 24.4 MiB, and CJK alone is 18.6 (CONFIRMED). Mapping CJK costs 4.25, failing its ≤ 3 bar, but still saves about 14.

## Validity
- **Renderer:** every r3/r4 log shows AMD RX 5700 XT (radeonsi), not llvmpipe; weston logs "Using GL renderer". frames ≥ 1 in every run.
- **Spread:** r1/r2 failed the 1 MiB rule on five variants. Under the amendment, back-to-back r3/r4 spread ≤ 0.7 everywhere.
- **Positive control:** ballast − barelist = **20.45** (bar 20 ± 2). PASS.
- **fastframe:** 85.85, against 85.4 in 3a-GPU.

## PSS median, MiB (mean of r3/r4)
| Variant | PSS |
|---|---|
| bare (no list) | 58.85 |
| barelist | 61.45 |
| ballast | 81.90 |
| fontsonly | 85.85 |
| themetext | 62.00 |
| noicons | 85.20 |
| fastframe | 85.85 |
| nofallbacks | 62.45 |
| emoji | 86.80 |
| mmapcjk | 66.70 |

## Decomposition
Total: fastframe − barelist = 24.40.

| Part | Calculation | MiB |
|---|---|---|
| Fonts | fontsonly − barelist | 24.40 |
| Theme + text | themetext − barelist | 0.55 |
| Icons | fastframe − noicons | 0.65 |

The parts sum to 25.60, 1.20 off the total (bar ±4): PASS. Theme+text and icons are within run noise. The list costs 2.6 (barelist − bare).

The icons were never drawn: there's no SVG loader (`egui_extras` isn't in the graph), in the 85.4 build too. The resvg cost is unmeasured.

## Fallback fonts
fastframe − nofallbacks = **23.40**, against the inventory's 22.89 (bar ±2): **CONFIRMED**. `scan()` reads each face into a `Vec` that lives for the whole process.

Inventory (583 fonts installed, 25 faces chosen):
- **Han, kana and hangul:** `NotoSansCJK-Regular.ttc` face 2, 19,484,784 bytes.
- **24 other scripts:** `NotoSans*-Regular.ttf`, together 4,514,332 bytes; the largest is Arabic at 244,072.

The cost depends on the installed fonts.

## Recommendation: memory-map the fallback faces, CJK first
Lazy-loading doesn't help: the first CJK title costs 18.6. The mapped `.ttc` touched 3.8 MiB with CJK drawn and 0.8 MiB with none (smaps probe, exploratory). `madvise(RANDOM)` changed nothing.

**Saving:** about **−14 MiB** while CJK is drawn: the 18.6 heap copy minus mmapcjk − nofallbacks (4.25). About −18 when idle (0.8 touched). Nothing else maps the file here; sharing would lower PSS.

**Named recommendation: upstream, `Fallback` holds an `Mmap`** for all 25 faces, as fastframe-emoji already does with `Bytes::Mapped`. `mmapcjk` is its measured CJK part. Unmeasured whole; estimate 63–67.

fastframe has no switch for this. Its only lever, `system_fallbacks(false)`, drops all 25 faces. Re-adding a mapped CJK face (`FontData::from_static`; epaint clones `from_owned`) works, as `mmapcjk` shows. But face choice and the baseline tweak are `pub(crate)`, and Arabic, Hebrew, Thai and Indic are lost, which multilingual titles rule out.

Emoji adds 0.95. Its font is mapped, with 0.4 MiB touched. Painted pictures are unverified (only frame 1 logged). `budget.md` is unchanged.

## Re-run
```
cargo build --release --bins --manifest-path spikes/03-native-cdm-and-ui/ui-baseline/Cargo.toml
for b in bare barelist ballast fontsonly themetext noicons fastframe nofallbacks emoji mmapcjk; do for r in 1 2; do sg render -c "RUN_SUFFIX=-s0-rerun$r spikes/03-native-cdm-and-ui/run-3a-gpu.sh $b"; done; done
sg render -c 'spikes/03-native-cdm-and-ui/s0-smaps-probe.sh mmapcjk'
sg render -c 'env S0_NO_CJK_ROWS=1 spikes/03-native-cdm-and-ui/s0-smaps-probe.sh mmapcjk'
```
