# MusicKit JS host API contract (seed, spike 4, 2026-10-09)

This is the seed of the contract our JS runtime must honour so that Apple's musickit.js runs unmodified.
- **Source:** the V8 logged run `spikes/04-js-engine/results/v8-logged.json`, which has spec-correct `has`/`get` semantics. The QuickJS logged run touched the identical set of paths.
- **musickit.js:** sha256 `21908b72…012c`, version `3.2526.0-prerelease.x`.
- **Phases covered:** script evaluation, `MusicKit.configure` with a dummy developer token, and one `api.music()` catalog request (stubbed).
- **Not covered:** authorize, playback, hls.js, EME, MSE. The static section lists what those phases reference.

## Legend
- `get`/`has`: the host provided it and MusicKit used it.
- `get-missing`/`probe-missing`: MusicKit looked for it, the host did not provide it, and MusicKit carried on. These are feature detections. Missing EME and MSE simply means preview-only or no playback path at configure time.
- `call`/`construct`: invoked.
- Counts are per run.

## Hard requirements (throw if absent)
Proven with V8: without these, evaluation dies with `ReferenceError`.
- `Document` and `HTMLElement`: constructors with `.prototype`. They are dereferenced at module scope by the fullscreen helpers, `(e=>…)(Document)` and `(HTMLElement)`.
- Also exercised and needed for the observed path: `window`/`self` (as the global object), `document` (`head.querySelector`, `querySelector`, `querySelectorAll`, `readyState`, `createElement`, `dispatchEvent`, `cookie`), `navigator.userAgent`, `navigator.platform`, `location.hostname`/`hash`, `localStorage`, `sessionStorage`, `Event`, `setTimeout`, `atob`, `btoa`, `fetch` (it calls `fetch.bind`), `Headers`, `URL`.
- Not proven: whether removing an individual provided global throws. Only `Document` and `HTMLElement` were tested by absence.

## Observed accesses by area

### DOM

| Path | Access (count) |
|---|---|
| `global.location` | has 12, get 14 |
| `global.location.hostname` | get 9 |
| `global.Document` | has 2, get 2 |
| `global.HTMLElement` | has 1, get 1 |
| `global.document` | has 24, get 24 |
| `global.document.mozHidden` | get-missing 1 |
| `global.document.msHidden` | get-missing 1 |
| `global.document.webkitHidden` | get-missing 1 |
| `global.document.querySelectorAll` | get 4, call 2 |
| `global.document.querySelector` | get 1, call 1 |
| `global.document.head` | get 6 |
| `global.document.head.querySelector` | get 6, call 6 |
| `global.document.readyState` | get 1 |
| `global.document.createElement` | get 2, call 2 |
| `global.location.hash` | get 1 |

### media

| Path | Access (count) |
|---|---|
| `global.VTTCue` | get-missing 1 |
| `global.VTTRegion` | get-missing 1 |
| `global.HlsUnresolvedUriData` | probe-missing 1 |
| `global.HlsDaterangeUpdatedData` | probe-missing 1 |
| `element.video.webkitSupportsPresentationMode` | get-missing 1 |
| `global.document.pictureInPictureEnabled` | get-missing 1 |
| `global.navigator.mediaSession` | get-missing 1 |

### EME

| Path | Access (count) |
|---|---|
| `global.LicenseData` | probe-missing 1 |
| `global.navigator.requestMediaKeySystemAccess` | get-missing 3 |
| `global.WebKitMediaKeys` | get-missing 2 |
| `global.MSMediaKeys` | get-missing 2 |

### MSE

| Path | Access (count) |
|---|---|
| `global.ManagedSourceBuffer` | get-missing 1 |
| `global.ManagedMediaSource` | get-missing 1 |
| `global.MediaSource` | get-missing 1 |

### network

| Path | Access (count) |
|---|---|
| `global.Headers` | has 3, get 3, construct 1 |
| `global.fetch` | has 6, get 7, call 1 |
| `global.fetch.bind` | get 3, call 3 |
| `global.navigator.connection` | get-missing 1 |
| `global.URL` | has 4, get 4 |

### storage

| Path | Access (count) |
|---|---|
| `global.localStorage` | has 30, get 30 |
| `global.localStorage.getItem` | get 16, call 16 |
| `global.sessionStorage` | has 2, get 2 |
| `global.document.cookie` | get 1 |
| `global.localStorage.removeItem` | get 1, call 1 |

### events

| Path | Access (count) |
|---|---|
| `global.onpagehide` | probe-missing 1 |
| `global.Event` | has 4, get 4, construct 2 |
| `global.document.dispatchEvent` | get 2, call 2 |

### timers

| Path | Access (count) |
|---|---|
| `global.setTimeout` | has 2, get 2, call 2 |

### misc

| Path | Access (count) |
|---|---|
| `global.exports` | probe-missing 1 |
| `global.define` | probe-missing 1 |
| `global.MusicKit` | set 1, has 8, get 8 |
| `global.self` | has 2, get 2 |
| `global.FastBoot` | probe-missing 1, get-missing 15 |
| `global.process` | probe-missing 2, get-missing 18 |
| `global.window` | has 27, get 27 |
| `global.Buffer` | get-missing 1 |
| `global.ActivityIntention` | probe-missing 2 |
| `global.ActivityNotificationOptions` | probe-missing 1 |
| `global.MusicKit.version` | get 1 |
| `global.MusicKit.configure` | get 2, call 1 |
| `global.btoa` | has 2, get 2, call 2 |
| `global.navigator` | get 12, has 7 |
| `global.navigator.userAgent` | get 5 |
| `global.atob` | get 2, call 2 |
| `global.name` | get-missing 1 |
| `global.navigator.platform` | get 1 |

### Network shape observed
- **Request:** `GET https://api.music.apple.com/v1/catalog/us/songs/1440833098`.
- **Headers:** names only were logged: `authorization`.
- **How it is built:** `new Headers` and `new URL`, with `fetch` bound to `window`.
- **`configure`:** made **no** request.

### Storage keys read or written (names only)
- localStorage get: `mk-console-logging`, `mk-logging-levels`, `mk-mse-buffer`, `mk-hlsjs-item-preloading`, `mk-block-report-cdn-server`, `mk-hlsjs-version`, `mk-hlsjs-log-level`, `mk-hlsjs-live-radio`, `mk-hlsjs-song-playback`, `mk-bag-features-overrides`, `music.<iss>.media-user-token`, `music.<iss>.itua`, `mk-use-paf-qa-endpoint`.
- localStorage remove: `music.<iss>.media-user-token`. Here `<iss>` is the developer token's `iss` (team ID), lowercased.
- **For spike 2:** the user token is very likely injected by pre-filling `music.<teamid>.media-user-token`. It is unverified with a real token, and MusicKit removed the key when it was absent or invalid.

### Elements created
- `document.createElement("video")` was called twice, at configure time, for capability probes such as `webkitSupportsPresentationMode`.

## Static references not exercised yet
From reading `musickit.js`; these are needed later and are still unverified at runtime.
- **Auth:** `window.open` (6 references; authorize popup) and `postMessage` (1). Our product authorizes through a loopback page in the system browser instead, so the runtime may only need these to fail gracefully. To be confirmed in spike 2.
- **hls.js:** MusicKit loads `https://js-cdn.music.apple.com/hls.js/2.910.1/hls.js/hls.js`, via `bag.urls.hls`, by appending a `<script>` to `document.head` and waiting for `onload`/`onerror`.
  - Our host must fetch and evaluate Apple-hosted scripts appended that way. This is browser-equivalent and keeps the script unmodified.
  - hls.js adds its own API surface: MSE, possibly Workers, and timers.
- **EME:**
  - `navigator.requestMediaKeySystemAccess` with configs that include `audioCapabilities: [{contentType: 'audio/mp4;codecs="mp4a.40.2"'}]` and video `robustness: "SW_SECURE_CRYPTO"`.
  - It probes key systems in preference order: FairPlay through `WebKitMediaKeys`, PlayReady through `MSMediaKeys`, then standard EME, with PlayReady preferred when the bag sets `prefer-playready`.
  - Also `MediaKeys` and `MediaKeySystemAccess` globals, and the sessionStorage flag `mk-playready-cbcs-unsupported`.
- **MSE:** `MediaSource`/`ManagedMediaSource`, and `SourceBuffer` (11 references).
- **Misc:** `navigator.mediaSession` (4), `TextDecoder` (3) and `Request` (2).
