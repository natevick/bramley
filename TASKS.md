# Tasks

- [x] Spike 1 verification (Norm), done 2026-10-09: reproduced FAIL at 398.4 MiB PSS median (worker measured 397.9); sabotage run VOID; null sink removed.
- [x] Playback-path decision (Nate, 2026-10-09): run MusicKit JS unmodified in our own JS runtime with native media and DRM host objects backed by the Widevine CDM. Rules are in docs/compliance.md.
- [ ] MusicKit key (Nate): generate a MusicKit private key in the Apple Developer portal. Hand it over through the broker; never paste it into chat.
- [ ] Spike 2 (needs key): our own developer token plus a loopback user token play a FULL song in headless Chrome. Record the browser APIs MusicKit actually touches.
- [ ] Spike 3: bare eframe UI memory baseline, then a native Widevine CDM host (Chromium BSD header, interface version from the installed library) that decrypts and plays Google's test audio, with memory measured.
- [ ] Spike 4: JS engine comparison (QuickJS vs V8/deno_core). Pass bar: musickit.js loaded and configured within budget.
- [ ] Thinnest end-to-end slice: MusicKit JS in engine, then shims, then CDM, then Apple license, then audible sound. Do this before any UI work.
- [ ] Name: read the Apple Music Identity Guidelines, then propose names to Nate.
