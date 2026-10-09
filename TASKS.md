# Tasks

- [x] Spike 1 verification (Norm), done 2026-10-09: reproduced FAIL at 398.4 MiB PSS median (worker measured 397.9); sabotage run VOID; null sink removed.
- [x] Playback-path decision (Nate, 2026-10-09): run MusicKit JS unmodified in our own JS runtime with native media and DRM host objects backed by the Widevine CDM. Rules are in docs/compliance.md.
- [ ] MusicKit key (Nate): generate a MusicKit private key in the Apple Developer portal. Hand it over through the broker; never paste it into chat.
- [ ] Spike 2 (needs key): our own developer token plus a loopback user token play a FULL song in headless Chrome. Record the browser APIs MusicKit actually touches.
- [x] Spike 3 (2026-10-09, reproduced by Norm): native Widevine host PASS at 86.8 MiB (66 s decrypted and decoded; sabotage VOID, 404). UI baseline 107 MiB under Xvfb/llvmpipe (FAIL as measured; ~41 MiB is llvmpipe). Real-GPU UI unmeasured: this box lacks render-node access.
- [x] Spike 4 (2026-10-09, reproduced by Norm): QuickJS-NG 8.5 MiB / V8 32-33 MiB PSS idle with musickit.js configured; sabotage VOID. Pick QuickJS; V8 is the fallback and API-contract recorder. Idle only: playback memory with hls.js is unmeasured.
- [x] Spike 5 (2026-10-09, reproduced by Norm): hls.js in QuickJS, through native MSE/EME host objects, into the CDM: PASS at 96.7 MiB, one process (bar 105); sabotage VOID. Not yet covered: pause, volume, eviction, musickit.js, Apple license server.
- [ ] Real-GPU UI measurement: this box gives no render-node access (norm is not in the `render` group). Needs Nate: add the group, or measure on his Linux machine.
- [ ] Engine crate (developer agent, branch `natevick/engine-crate`): graduate spike 5 into `crates/engine` with pause, seek, volume and eviction, plus tests. When it reports, Norm re-runs `scripts/measure-playback.sh`, the sabotage run and the pause/seek scenario before merging locally.
- [ ] Thinnest end-to-end slice: MusicKit JS in engine, then shims, then CDM, then Apple license, then audible sound. Do this before any UI work.
- [ ] Name: read the Apple Music Identity Guidelines, then propose names to Nate.
