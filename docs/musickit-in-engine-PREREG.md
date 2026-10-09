# MusicKit JS in the Bramley engine (pre-registered 2026-10-09)

**Question.** Does Apple's unmodified musickit.js, fetched from Apple's CDN at runtime, play a FULL song inside `bramley-engine` with no browser? The engine runs QuickJS, our web-platform host objects, the native Widevine CDM and PipeWire output.

**Pass bar.** Spike 2's checks, applied to the engine process. All are required, or the verdict is FAIL.
1. `isAuthorized === true` and the storefront is `us`.
2. Bohemian Rhapsody (6781024437, 355 s) passes 35 s on the JS-visible `currentPlaybackTime` and keeps advancing. The native side decodes more than 35 s from DECRYPTED samples, counted separately from any clear lead-in.
3. The CDM reports a usable key. Record the license host (only the host) and the scheme.
4. A PipeWire stream is linked to a temporary null sink, and the default sink is unchanged.
5. **Memory:** whole-process PSS median over 60 s of steady playback.
   - **Pass:** ≤ 112 MiB. That is 200 − 85.4 for the UI, rounded down.
   - **Marginal:** 112–125 MiB.
   - **Fail:** above 125 MiB.
6. Pause, seek to 120 s, resume, then skip to the next item. Each step lands within 0.5 s and the engine does not crash.

**Sabotage (must not PASS).**
- **A:** no user token. The 30 s preview must stop check 2.
- **B:** a corrupted developer token.
- **C:** musickit.js truncated in memory. `MusicKit` stays undefined and the run is VOID.

**Compliance gates (any violation stops the work and is reported to Nate):**
- musickit.js and any scripts it loads come from Apple's CDN and are evaluated byte-for-byte. Record the sha256 of the bytes fetched and evaluated.
- Never call Apple's private playback or license endpoints from our code; only MusicKit does that, through our fetch host object.
- The Origin, User-Agent and document location we present must be honest and Bramley's own. If Apple rejects them, stop and do not spoof.
- No token, license payload or decrypted audio is written anywhere.

**Named false positives.**
- A preview counted as a full song. Check 2 and sabotage A guard this.
- The JS clock advancing while native output starves. Check 2 requires both.
- A stub that silently no-ops an interface MusicKit needed. Log every host method called and every unknown global touched.
