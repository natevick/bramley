# S0 pre-registration: where fastframe's +26 MiB goes (2026-10-10)

Written before any S0 variant was built or measured.

## Question
Spike 3a-GPU measured fastframe at 85.4 MiB PSS against 59.1 for bare eframe, on the real GPU. Which component accounts for the +26, and what can be dropped, loaded lazily or memory-mapped without forking fastframe?

**Lead:** `fastframe-fonts` `system::scan()` reads each chosen fallback face with `std::fs::read` into a `Vec<u8>` held in a `static OnceLock`, and epaint borrows it through `FontData::from_static`. That is one fully touched heap copy per face. On this box `NotoSansCJK-Regular.ttc` is 19.5 MB.

## Variants
All are bins in `spikes/03-native-cdm-and-ui/ui-baseline`, measured with `run-3a-gpu.sh` (weston headless `--renderer=gl`, 5 s warmup, 30 s sample, PSS median of the process tree). Every list variant draws the same 1,000-row list as the 85.4 build, with Latin titles only.

| Bin | Contents |
|---|---|
| `bare` | the 3a bare window, no list (reference only) |
| `barelist` | bare plus the 1,000-row list, egui's default fonts |
| `ballast` | `barelist` plus 20 × 1 MiB buffers filled with a non-zero byte, held in the app (positive control) |
| `fontsonly` | `barelist` plus `FontSetup::default()` (Inter and system fallbacks) |
| `themetext` | `barelist` plus the Tokyo theme and `fastframe_text::detect()` |
| `noicons` | full fastframe without icons |
| `fastframe` | full: fonts, theme, text, icons and list (the 85.4 build, refactored) |
| `nofallbacks` | full with `system_fallbacks(false)` |
| `emoji` | full plus `fastframe-emoji`: `EmojiSetup::default().install()`, `warm_up`, `EmojiPlugin`, and visible emoji in the header |
| `mmapcjk` | `nofallbacks` plus the CJK `.ttc` memory-mapped and pushed as the last fallback of every family; every 10th title is CJK, so the visible rows draw CJK glyphs |

Each bin prints one `SPIKE components=… fallback_faces=N fallback_bytes=B` line, and a stderr logger captures fastframe's `info` lines, which name each fallback file.

## Validity (any failure makes the run VOID)
1. The `gl_renderer` line names the AMD GPU (radeonsi), not llvmpipe, and weston's log says "Using GL renderer".
2. `frames` > 0 for every run.
3. Two runs per variant, with PSS medians within 1 MiB of each other. The table reports the mean of the two.
4. **Positive control:** `ballast − barelist` = 20 ± 2 MiB.
5. `fastframe` falls within 2 MiB of 85.4. If it doesn't, explain the gap before decomposing anything.

## Decomposition check
Total T = `fastframe − barelist`. The add-one deltas are measured independently, so they don't telescope:

- F = `fontsonly − barelist`
- X = `themetext − barelist`
- I = `fastframe − noicons`

**PASS** if |F + X + I − T| ≤ 4 MiB. Otherwise the components interact, and RESULTS says so; no per-component claim is made.

## CJK / fallback hypothesis
Let S = Σ `fallbacks()[i].bytes.len()` in MiB, from the inventory.

- **CONFIRMED** if `fastframe − nofallbacks` = S ± 2.
- **REFUTED** otherwise. RESULTS then names where the fonts' cost actually goes.

The CJK share is reported as the `.ttc` length against S.

## Recommendation variant
**PASS** if `mmapcjk − nofallbacks` ≤ 3 MiB while CJK titles are on screen. The predicted saving is therefore at least the CJK length minus 3.

The emoji delta (`emoji − fastframe`) is reported with no threshold. Bramley doesn't use emoji yet.

## Out of scope
- Album art.
- Font inventories on other machines. The cost depends on the installed fonts, and RESULTS states this box's inventory.
- The budget row changes only if a measured number supersedes 85.4 for the shipped UI configuration.
