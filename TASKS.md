# Tasks

- [x] Spike 1 verification (Norm), done 2026-10-09: reproduced FAIL at 398.4 MiB PSS median (worker measured 397.9); sabotage run VOID; null sink removed.
- [x] Playback-path decision (Nate, 2026-10-09): run MusicKit JS unmodified in our own JS runtime with native media and DRM host objects backed by the Widevine CDM. Rules are in docs/compliance.md.
- [ ] MusicKit key (Nate): generate a MusicKit private key in the Apple Developer portal. Hand it over through the broker; never paste it into chat.
- [ ] Spike 2 (needs key): our own developer token plus a loopback user token play a FULL song in headless Chrome. Record the browser APIs MusicKit actually touches.
- [x] Spike 3 (2026-10-09, reproduced by Norm): native Widevine host PASS at 86.8 MiB (66 s decrypted and decoded; sabotage VOID, 404). UI baseline 107 MiB under Xvfb/llvmpipe (FAIL as measured; ~41 MiB is llvmpipe). Real-GPU UI unmeasured: this box lacks render-node access.
- [x] Spike 4 (2026-10-09, reproduced by Norm): QuickJS-NG 8.5 MiB / V8 32-33 MiB PSS idle with musickit.js configured; sabotage VOID. Pick QuickJS; V8 is the fallback and API-contract recorder. Idle only: playback memory with hls.js is unmeasured.
- [x] Spike 5 (2026-10-09, reproduced by Norm): hls.js in QuickJS, through native MSE/EME host objects, into the CDM: PASS at 96.7 MiB, one process (bar 105); sabotage VOID. Not yet covered: pause, volume, eviction, musickit.js, Apple license server.
- [ ] Real-GPU UI measurement: this box gives no render-node access (norm is not in the `render` group). Needs Nate: add the group, or measure on his Linux machine.
- [ ] Engine crate, branch `natevick/engine-crate` at 01915a4. Norm reproduced it 2026-10-09: 97.6 MiB PASS, sabotage VOID, pause/seek PASS (with sabotage FAIL), 34 tests plus the e2e passing. MERGE GATE: the reviewer report at `scratch/review-engine-crate.md`. Fix any blocker or major finding on the branch and re-run `scripts/measure-playback.sh` plus the scenario, then fast-forward main.
- [ ] Thinnest end-to-end slice: MusicKit JS in engine, then shims, then CDM, then Apple license, then audible sound. Do this before any UI work.
- [ ] Name (Nate 2026-10-09: "Orchard" dropped because The Orchard is a music distributor). A research agent is checking candidates in `scratch/naming/names.md`; propose 3–4 to Nate.
- [ ] License and open source (Nate 2026-10-09): MIT, the same as Spotifast, with everything public in one repo. After the engine merge: add LICENSE (MIT, Nate Vick), rename crates and the directory to the chosen name, and ask Nate before creating the public GitHub repo.
