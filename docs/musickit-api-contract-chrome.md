# MusicKit JS host API contract, observed in Chrome (spike 2, 2026-10-09)

This lists every instrumented web API member that Apple's code called during a real, authorized, full-length playback session in headless Chrome 154.
- **Session:** load, configure, play Bohemian Rhapsody (flavor `28:ctrp256`), pause, seek to 120 s, resume, then skip to the next song.
- **Coverage:** 104 members; 31 also appear in the spike-4 seed (✓ means the member name is in `docs/musickit-api-contract-seed.md`).
- **Method:** logging getters and method wrappers installed through `Page.addScriptToEvaluateOnNewDocument` on the prototypes listed in `spikes/02-musickit-surface/harness/instrument.js`, plus window globals. Spike-only, never shipped.
- **Attribution:** an access is attributed to Apple's code when the stack contains `musickit/v3` or hls.js.
- **Limits:** this is NOT the complete contract.
  - `in`-operator feature probes are invisible.
  - Non-configurable members such as `window.document` and `window.location` cannot be wrapped; their members on the prototypes are.
  - Workers are not instrumented.
  - The spike-4 seed remains the record of configure-time probes.

| Area | Member | Accesses | Attributed to musickit (first 40 sampled) | hls.js | In seed |
|---|---|---|---|---|---|
| EME | `MediaKeySession.close()` | 1 | 1 | 0 |  |
| EME | `MediaKeySession.generateRequest()` | 2 | 2 | 0 |  |
| EME | `MediaKeySession.remove()` | 1 | 1 | 0 |  |
| EME | `MediaKeySession.sessionId` | 8 | 8 | 0 |  |
| EME | `MediaKeySession.update()` | 2 | 2 | 0 |  |
| EME | `MediaKeySystemAccess.createMediaKeys()` | 1 | 1 | 0 |  |
| EME | `MediaKeySystemAccess.keySystem` | 1 | 1 | 0 |  |
| EME | `MediaKeys.createSession()` | 2 | 2 | 0 |  |
| EME | `MediaKeys.setServerCertificate()` | 1 | 1 | 0 |  |
| EME | `Navigator.requestMediaKeySystemAccess()` | 8 | 8 | 0 | ✓ |
| MSE | `MediaSource.activeSourceBuffers` | 2 | 2 | 0 |  |
| MSE | `MediaSource.addSourceBuffer()` | 2 | 2 | 0 |  |
| MSE | `MediaSource.endOfStream()` | 2 | 2 | 0 |  |
| MSE | `MediaSource.readyState` | 4 | 4 | 0 | ✓ |
| MSE | `SourceBuffer.appendBuffer()` | 91 | 40 | 0 |  |
| MSE | `SourceBuffer.appendWindowStart` | 92 | 40 | 0 |  |
| MSE | `SourceBuffer.buffered` | 11 | 11 | 0 |  |
| MSE | `SourceBuffer.remove()` | 6 | 6 | 0 |  |
| MSE | `SourceBuffer.timestampOffset=` | 1 | 1 | 0 |  |
| MSE | `SourceBuffer.updating` | 9 | 9 | 0 |  |
| MSE | `TimeRanges.end()` | 26 | 26 | 0 |  |
| MSE | `TimeRanges.start()` | 35 | 35 | 0 |  |
| MSE | `window.MediaSource` | 3 | 3 | 0 | ✓ |
| MSE | `window.SourceBuffer` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.autoplay=` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.buffered` | 107 | 40 | 0 |  |
| Media | `HTMLMediaElement.canPlayType()` | 3 | 3 | 0 |  |
| Media | `HTMLMediaElement.controls=` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.currentTime` | 1249 | 32 | 0 |  |
| Media | `HTMLMediaElement.currentTime=` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.defaultPlaybackRate=` | 2 | 2 | 0 |  |
| Media | `HTMLMediaElement.duration` | 3 | 3 | 0 |  |
| Media | `HTMLMediaElement.load()` | 202 | 40 | 0 |  |
| Media | `HTMLMediaElement.muted=` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.pause()` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.paused` | 38 | 1 | 0 |  |
| Media | `HTMLMediaElement.play()` | 3 | 3 | 0 |  |
| Media | `HTMLMediaElement.playbackRate=` | 3 | 3 | 0 |  |
| Media | `HTMLMediaElement.preload=` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.setMediaKeys()` | 1 | 1 | 0 |  |
| Media | `HTMLMediaElement.src=` | 2 | 2 | 0 |  |
| Media | `HTMLMediaElement.volume=` | 3 | 3 | 0 |  |
| Media | `MediaSession.metadata=` | 2 | 2 | 0 |  |
| Media | `MediaSession.setActionHandler()` | 26 | 26 | 0 |  |
| Media | `Navigator.mediaSession` | 1 | 1 | 0 | ✓ |
| Media | `window.MediaMetadata` | 2 | 2 | 0 |  |
| Network | `Headers.append()` | 91 | 40 | 0 |  |
| Network | `Headers.set()` | 24 | 24 | 0 |  |
| Network | `Navigator.connection` | 2 | 2 | 0 | ✓ |
| Network | `NetworkInformation.downlink` | 2 | 2 | 0 |  |
| Network | `Request.method` | 1 | 1 | 0 |  |
| Network | `Request.url` | 1 | 1 | 0 |  |
| Network | `Response.arrayBuffer()` | 92 | 40 | 0 |  |
| Network | `Response.json()` | 4 | 4 | 0 |  |
| Network | `Response.ok` | 3 | 3 | 0 |  |
| Network | `Response.text()` | 11 | 6 | 0 |  |
| Network | `URL(static).createObjectURL()` | 2 | 2 | 0 |  |
| Network | `URL(static).revokeObjectURL()` | 1 | 1 | 0 |  |
| Network | `URL.host` | 109 | 40 | 0 |  |
| Network | `URL.pathname` | 218 | 40 | 0 |  |
| Network | `window.Headers` | 101 | 40 | 0 | ✓ |
| Network | `window.Request` | 1 | 1 | 0 |  |
| Network | `window.URL` | 116 | 40 | 0 | ✓ |
| Network | `window.fetch` | 107 | 40 | 0 | ✓ |
| Storage | `Document.cookie` | 1 | 1 | 0 | ✓ |
| Storage | `Storage.getItem()` | 26 | 26 | 0 | ✓ |
| Storage | `Storage.setItem()` | 3 | 3 | 0 |  |
| Storage | `window.localStorage` | 40 | 40 | 0 | ✓ |
| Storage | `window.sessionStorage` | 21 | 21 | 0 | ✓ |
| Events | `Event.target` | 2 | 2 | 0 |  |
| Events | `Event.type` | 577 | 40 | 0 |  |
| Events | `EventTarget.addEventListener()` | 32 | 31 | 0 |  |
| Events | `EventTarget.dispatchEvent()` | 2 | 2 | 0 | ✓ |
| Events | `EventTarget.removeEventListener()` | 5 | 5 | 0 |  |
| Events | `window.Event` | 4 | 4 | 0 | ✓ |
| Timers | `window.clearTimeout` | 2 | 2 | 0 |  |
| Timers | `window.setTimeout` | 5 | 5 | 0 | ✓ |
| DOM | `Document.body` | 1 | 1 | 0 |  |
| DOM | `Document.createElement()` | 202 | 40 | 0 | ✓ |
| DOM | `Document.head` | 6 | 6 | 0 | ✓ |
| DOM | `Document.pictureInPictureEnabled` | 1 | 1 | 0 | ✓ |
| DOM | `Document.querySelector()` | 1 | 1 | 0 | ✓ |
| DOM | `Document.querySelectorAll()` | 2 | 2 | 0 | ✓ |
| DOM | `Document.readyState` | 1 | 1 | 0 | ✓ |
| DOM | `Document.webkitHidden` | 1 | 1 | 0 | ✓ |
| DOM | `Element.id` | 1 | 1 | 0 |  |
| DOM | `Element.id=` | 1 | 1 | 0 |  |
| DOM | `Element.querySelector()` | 6 | 6 | 0 | ✓ |
| DOM | `HTMLElement.dataset` | 1 | 1 | 0 |  |
| DOM | `HTMLElement.title=` | 2 | 2 | 0 |  |
| DOM | `HTMLScriptElement.src` | 2 | 2 | 0 |  |
| DOM | `Node.appendChild()` | 1 | 1 | 0 |  |
| DOM | `Node.parentNode` | 5 | 5 | 0 |  |
| DOM | `window.Document` | 2 | 2 | 0 | ✓ |
| DOM | `window.HTMLElement` | 6 | 6 | 0 | ✓ |
| DOM | `window.devicePixelRatio` | 12 | 12 | 0 |  |
| Misc | `Navigator.platform` | 1 | 1 | 0 | ✓ |
| Misc | `Navigator.userAgent` | 12 | 12 | 0 | ✓ |
| Misc | `window.MediaKeySystemAccess` | 4 | 4 | 0 |  |
| Misc | `window.MediaKeys` | 4 | 4 | 0 |  |
| Misc | `window.VTTCue` | 4 | 4 | 0 | ✓ |
| Misc | `window.atob` | 6 | 6 | 0 | ✓ |
| Misc | `window.navigator` | 37 | 37 | 0 | ✓ |
| Misc | `window.self` | 4 | 4 | 0 | ✓ |

## Observed playback facts that the runtime must honour

- **hls.js:** MusicKit did not use hls.js for songs. Flavor `28:ctrp256` arrives as HTTP byte-range (206) fetches of one fMP4 asset from `aod-ssl.itunes.apple.com`. Each fetch is appended through MSE as `audio/mp4;codecs="mp4a.40.2"` (AAC-LC), and no m3u8 was fetched.
- **Encryption:** the `schm` scheme is `cenc`, the original format (`frma`) is `mp4a`, and the init data type is `cenc`. The PSSH carries the Widevine system id `edef8ba9-79d6-4ace-a3c8-27dcd51d21ed`.
- **Key systems:** probed in order: `com.microsoft.playready` and `com.youtube.playready` (both rejected in Chrome), then `com.widevine.alpha`.
  - The first Widevine call asks for `videoCapabilities` `avc1.42E01E` with `SW_SECURE_CRYPTO`, plus `mp4a.40.2` audio.
  - A later call is audio-only.
  - Both ask `persistentState: "required"` and `distinctiveIdentifier: "optional"`.
  - **Our runtime must grant `persistentState: "required"`.** Back it with in-memory CDM storage. Then check whether MusicKit tolerates the video config being refused.
- **`MediaKeys.setServerCertificate()` is required.** MusicKit fetches the certificate from `play.itunes.apple.com/WebObjects/MZPlay.woa/wa/widevineCert`. Spike 5 and the engine never exercised this.
- **License:** MusicKit itself POSTs to `play.itunes.apple.com/WebObjects/MZPlay.woa/wa/acquireWebPlaybackLicense` through `fetch`. Our code never calls it (compliance rule 4).
- **Other Apple endpoints MusicKit calls:**
  - `MZPlay.woa/wa/webPlayback` (POST)
  - `universal-activity-service.itunes.apple.com/play` (play-activity reporting, POST)
  - `api.music.apple.com/v1/catalog/{sf}/songs`
  - `api.music.apple.com/v1/me/account`
- **`isAuthorized` is not proof of a valid sign-in.** It only reflects that a user token is stored: with a corrupted developer token it still read `true`.
- **Previews:** with no user token, MusicKit plays the preview, which stops at exactly 30.0 s.
