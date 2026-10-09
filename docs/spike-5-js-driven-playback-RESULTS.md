# Spike 5 results: JS-driven DRM playback in one process (2026-10-09)

The pre-registration is `spike-5-js-driven-playback-PREREG.md`. The bar was ≤ 105 MiB PSS median over 60 s of steady playback. 105–125 counts as marginal; above 125 fails.

## Verdict: **PASS**

Upstream hls.js 1.7.3 runs in QuickJS-NG and drives our Rust host objects (media element, MSE, EME). Those host objects drive the spike-3 Widevine CDM through to PipeWire, all in one process.

| Run | PSS med (max) | RSS | USS | JS max `currentTime` | Decrypted s | Clear s | Verdict |
|---|---|---|---|---|---|---|---|
| 5-run1 | **96.6** (96.6) | 100.7 | 96.3 | 60.02 | 60.2 | 16.0 | PASS |
| 5-run2 | **97.2** (97.2) | 101.3 | 96.9 | 60.02 | 60.1 | 16.0 | PASS |
| 5-run3-final | **97.1** (97.1) | 101.3 | 96.9 | 59.98 | 60.4 | 16.0 | PASS |
| 5-sabotage-final | 89.2 | 93.3 | 88.9 | 7.79 | 0 | 8.0 | VOID |

- Values are MiB.
- Each run had 1 process with 2 threads, plus a short-lived thread per HTTP request.
- Each real run looped once at 60 s, which is a real seek to 0.

**Did-it-run checks.** All passed in every real run:
- JS saw `keystatuseschange` with five `usable` keys, and hls.js emitted `KEY_LOADED` 13 times.
- JS `currentTime` passed 30 s.
- More than 30 s was decoded from decrypted samples, counted apart from the clear lead-in.
- There were 0 decrypt failures and 0 decode errors.
- Our PID's PipeWire stream was linked to `spike5-null`.
- The default sink was unchanged.

**Sabotage proof.** With the license URL broken:
- hls.js got a 404 and raised a fatal `keySystemLicenseRequestFailed`.
- No key statuses came back.
- Only the 8 s clear lead-in played before playback stalled, so the run is VOID.
- An earlier run (`5-run1-preloopfix`) exposed an end-of-stream bug that left the last 15 s of the window idle. It was fixed and superseded.

## Content
- **Stream:** Shaka's `angel-one-widevine-hls` English stereo AAC-LC (CENC), licensed through `cwip-shaka-proxy.appspot.com/no_auth`. It is the only Widevine HLS asset in Shaka's catalog, and it is HLS like MusicKit's, so hls.js was used rather than shaka DASH.
- **Codec wrapper:** a one-line audio-only master playlist, served as a `data:` URL, declares `CODECS="mp4a.40.2"`.
  - Without it, hls.js asks EME for `avc1` video, its default when no codecs are declared, and our runtime correctly refuses video.
  - Real audio-only services declare codecs.

## Where the memory and CPU go (profile run, 96.7 MiB)
- **Widevine is about 81 MiB** (`.bss`, 19.9 MiB of code, `partition_alloc`). Everything else is about 15 MiB, of which the heap is 7.0.
- **CPU and startup:** 2.6 % of one core during playback (one run, not pre-registered). First audio came 0.8–1.07 s after a cold start.

## Host methods called (names only)
- **Media element:** `HTMLMediaElement.src=`, `.play`, `.currentTime` (about 3.7k reads), `.buffered`, `.setMediaKeys`
- **MSE:**
  - `MediaSource.constructor`, `.isTypeSupported`, `.addSourceBuffer`, `.duration=`, `.endOfStream`
  - `SourceBuffer.appendBuffer`, `.buffered`
  - `URL.createObjectURL`
- **EME:**
  - `navigator.requestMediaKeySystemAccess`, `MediaKeySystemAccess.createMediaKeys`, `MediaKeys.createSession`
  - `MediaKeySession.generateRequest`, `.update`
- **Network and DOM:** `XMLHttpRequest.open` and `.send`, `document.createElement`, `Node.appendChild`, `HTMLScriptElement.load`
- **Timers:** `setTimeout`, `setInterval` and their clears
- **Events dispatched:** `loadedmetadata`, `loadeddata`, `canplay`, `canplaythrough`, `play`, `playing`, `waiting`, `seeking`, `seeked`, `timeupdate`, `encrypted` (×2, which hls.js deduplicated into one session), `message`, `keystatuseschange`

## Caveats
- **This is a stand-in for Apple.** Apple's hls.js fork, musickit.js and Apple's license server are all untested.
- **The platform layer is JavaScript over about 40 native ops** (the Deno model), evaluated separately from page scripts. hls.js was loaded through `<script>` and evaluated unmodified. Whether compliance requires native classes is Nate's call.
- **The runtime identifies itself honestly.** The origin is `app://spike5` and the user agent is `spike5-engine`. There is no CORS, and `localStorage` is in memory only.
- **Not exercised or simplified:** pause, volume and back-buffer eviction were not exercised, and `currentTime` ignores output latency. Output and HTTP use spike-grade crates (a pure-Rust PulseAudio client and ureq).
- **No Kopuz or cdm-host code was used.**

## Re-run
```
cd /home/norm/projects/Personal/bramley/spikes/05-js-driven-playback
(cd engine && CARGO_TARGET_DIR=$PWD/target cargo build --release)
WARMUP=15 SAMPLE=60 ./run-5.sh 5-verify
WARMUP=15 SAMPLE=60 ./run-5.sh 5-sabotage-verify --lic https://cwip-shaka-proxy.appspot.com/no_auth_SABOTAGED
./run-5-profile.sh 5-profile-verify
```
