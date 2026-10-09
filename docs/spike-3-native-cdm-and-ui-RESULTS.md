# Spike 3 results (2026-10-09)

Pre-registration: `spike-3-native-cdm-and-ui-PREREG.md`. Values below are MiB, PSS summed over the process tree (one process in both parts).

## 3a: UI baseline. **FAIL (as measured, under llvmpipe)**

| Build | PSS median | RSS | USS |
|---|---|---|---|
| bare eframe glow, run 1 / run 2 | 85.8 / 82.0 | 113.0 / 108.3 | 63.0 / 60.1 |
| **fastframe** (fonts, theme, icons, text, 1,000-row list), run 1 / run 2 | **109.6 / 108.6** | 136.9 / 134.9 | 86.9 / 86.7 |

The bar was ≤ 70 to pass and > 100 to fail.

- **Did-it-run:** the renderer logged as `llvmpipe (LLVM 21.1.8)`, Mesa 26.0.8, and frames were presented.
- **The harness can fail:** the first attempt crashed (`libxkbcommon-x11` missing) and reported 0 samples, not a number. The library was extracted locally from Ubuntu's .deb into `syslibs/`; nothing was installed system-wide.
- **Breakdown:** libLLVM 31.8, anon 27.7, heap 24.2, libgallium 9.5, binary 9.3. About 41 MiB is the llvmpipe software renderer's libraries.
- **Real GPU:** not measured. The AMD render node returns EACCES because this user is not in the `render` group.

## 3b: native Widevine CDM host. **PASS**

| Run | PSS median (max) | RSS | USS | Decrypted audio decoded | Decrypt fails | Decode errors |
|---|---|---|---|---|---|---|
| run1 | **86.0** (86.1) | 90.8 | 85.5 | 66.0 s, 3,096 samples | 0 | 0 |
| run2 | **86.6** (86.9) | 90.6 | 86.3 | 66.0 s, 3,094 samples | 0 | 0 |
| sabotage | 80.1 | | | 0 s | 50 × kNoKey | |

Verdicts: run1 and run2 PASS (bar ≤ 90); sabotage VOID.

- **CDM:** 4.10.3112.0, loaded in place from `/opt/google/chrome/WidevineCdm`. It exports `InitializeCdmModule_4`, `CreateCdmInstance`, `GetCdmVersion`, `DeinitializeCdmModule` and `VerifyCdmHost_0`.
  - Interface 10 was created and requested Host_10.
  - `VerifyCdmHost_0` was never called and wasn't needed.
- **Pipeline:** fMP4 demux → CENC `Decrypt` (samples return to the host) → symphonia AAC → pure-Rust PulseAudio client → PipeWire.
- **License:** HTTP 200 (1,822-byte challenge, 1,229-byte response); all five keys usable.
- **What the CDM asked of the host:** only `QueryOutputProtectionStatus`, answered truthfully ("no display link") and logged. No FileIO, storage ID or platform challenge.
- **Checks passed in both real runs:**
  - key usable;
  - more than 30 s decoded from decrypted samples;
  - our PID's PipeWire stream linked to `spike3-null`;
  - tree walked;
  - default sink unchanged.
- **Sabotage:** the license URL returned 404 and no keys came back. Only the 12 s clear lead-in played; every encrypted sample failed with kNoKey.
- **Breakdown:**
  - **About 81 MiB is Widevine itself:** its 59 MiB `.bss` (57 resident), 20.1 MiB of code, and 3.8 MiB of partition_alloc.
  - Our host adds about 4–5 MiB.
  - With no key loaded, the sabotage run still sat at 80, so the cost is paid at initialization.

## Caveats
- **3b's headroom is 4 MiB, and nearly all of 3b is Widevine's fixed cost.**
- **3a is software rendering only.** The real-GPU UI number is the open question.
- **Output and HTTP aren't the product stack.** Output used a pure-Rust PulseAudio client rather than fastframe-audio/cpal (no ALSA headers, no sudo). HTTP used ureq.
- **Clean provenance:** no Kopuz or cdm-host code was consulted. The host is written against Chromium's BSD `content_decryption_module.h` (header kept, plus `LICENSE.chromium`).

## Re-run
```
cd /home/norm/projects/Personal/bramley/spikes/03-native-cdm-and-ui
(cd ui-baseline && CARGO_TARGET_DIR=$PWD/target cargo build --release)
(cd cdm-host && CARGO_TARGET_DIR=$PWD/target cargo build --release)
RUN_SUFFIX=-verify ./run-3a.sh bare fastframe
./run-3b.sh 3b-verify
./run-3b.sh 3b-sabotage-verify --lic https://cwip-shaka-proxy.appspot.com/no_auth_SABOTAGED
./run-3a-breakdown.sh fastframe
```
`run-3b.sh` prints the verdict JSON and also writes `results/<name>.verdict.json`.
