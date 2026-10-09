# Spike 5: JS-driven DRM playback in one native process (pre-registered 2026-10-09)

**Question.** Can a standard JS media player, running in QuickJS-NG, drive our native host objects (media element, MSE and EME) through to the native Widevine CDM from spike 3? It must play encrypted audio audibly, and the playback engine must fit in memory.

**Why this player.** Upstream hls.js (open source, a pinned version from jsdelivr) stands in for MusicKit's own hls.js fork until the MusicKit key arrives. It exercises the same web interfaces MusicKit's player uses. No Apple endpoints are contacted.

**Content.** A public Widevine-protected HLS (fMP4/CENC) audio stream with a no-auth license proxy, e.g. Shaka's demo assets. The worker picks it and records the URL. If no HLS variant exists, use DASH via shaka-player instead and record that.

**Architecture under test.** One process:
- QuickJS runs the player script, which is ordinary page logic.
- Host objects are implemented in Rust:
  - HTMLMediaElement subset
  - MediaSource and SourceBuffer
  - `navigator.requestMediaKeySystemAccess`, MediaKeys and MediaKeySession
  - fetch and XHR
  - timers and events
- Behind the host objects: fMP4 demux, CDM decrypt, AAC decode, PulseAudio/PipeWire output.

**Pass.** Whole-process PSS median ≤ 105 MiB over 60 s of steady playback. That is the CDM's 87 plus the JS engine's 8.5, plus about 10 for the player and buffers.
- Marginal: 105–125.
- Fail: above 125.

**Did-it-run checks (all required, or the run is void).**
1. JS saw a `keystatuseschange` with status `usable` through the EME API.
2. The JS-visible `media.currentTime` passed 30 s, and the native side decoded more than 30 s from decrypted samples, counted separately from clear lead-in.
3. A PipeWire stream node is linked to a temporary null sink.
4. A sabotage run with a broken license URL reads VOID.
5. The process count is 1, or the tree is walked.

**Named false positives.**
- The JS player believes it is playing while the native side starves. Check 2 requires both sides.
- Clear lead-in samples counted as DRM. Check 2 counts them separately.
- Stubs that no-op an interface the player needed, so audio comes from somewhere else. Log every host method called.
