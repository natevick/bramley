# Native Apple Music playback on Linux/Windows (2026-10-09)

**[src]** = I read the primary source or code · **[2nd]** = a secondary report · **[unverified]** = nothing confirmed. I ran none of these apps.

## 1. Which DRM non-Safari browsers use
- MusicKit JS v3 can use FairPlay, PlayReady or Widevine.
- For Widevine it asks for `"WIDEVINE_SOFTWARE"` and sets no robustness requirement on audio, so **the software L3 level is enough** [src: https://js-cdn.music.apple.com/musickit/v3/musickit.js].
- **It works on Linux Chrome today.** A Chromium 152 media log from Sept 2026 shows Widevine with CENC encryption delivering AAC 44.1 kHz audio [2nd, measured: https://marius.blog/sidra-apple-music-linux/].
- Linux has no VMP, Apple serves it anyway; no reports of an L1/VMP requirement.
- **Lossless conflicts between sources:** Sidra claims it on VMP-signed macOS/Windows builds; Cider-2 and amtui say the web path is AAC 256 only.
- Firefox is probably fine [unverified].

## 2. The playback flow
- **Headers** [src: musickit.js]: `Authorization: Bearer <developer token>` plus `X-Apple-Music-User-Token`.
- **musickit.js v3 contains two song paths:** `MZPlay.woa/wa/webPlayback` (a POST with `{salableAdamId}`) and `api.music.apple.com/v1/play/assets?includeLicenseUrls=true`. **Which one songs use today is unverified.**
- **What `webPlayback` returns** [src: https://github.com/glomatico/gamdl, Kopuz]: `songList[0].assets[]`, each with a `flavor`, an HLS `URL` and key URLs. The flavors:
  - `28:ctrp256`: AAC-LC 256, CENC, with Widevine pssh data in `EXT-X-KEY`.
  - `32:ctrp64`: HE-AAC.
  - `30:cbcp256`: FairPlay, cbcs.
  - No ALAC on the web path (`"enhanced-hls": false`).
- **License request:** a POST to the returned `hls-key-server-url` (observed: `…/acquireWebPlaybackLicense`).
  - Body: `{challenge, "key-system", uri, adamId, isLibrary, "user-initiated"}`.
  - The reply is JSON with `status:0` on success.
- **Rippers:** gamdl uses Widevine through pywidevine with an extracted device key. The "wrapper" tool handles FairPlay (ALAC), reportedly via extracted Android libraries [unverified].

## 3. Hosting the Widevine CDM outside a browser
- **Kodi's inputstream.adaptive** [src: https://github.com/xbmc/inputstream.adaptive]:
  - Supports CDM interface versions 10, 11 and 12.
  - For audio it calls `Decrypt()`, which **returns clear samples to the host**. Only video can use the CDM's built-in decode path.
  - inputstreamhelper downloads the CDM from Google after an EULA prompt; never redistributed.
- **Rust:**
  - `cdm-host` (https://github.com/Kopuz-org/cdm-host) is a host using the `Host_11` interface. It borrows the browser's CDM.
  - **Kopuz** (https://github.com/Kopuz-org/kopuz) does the same, and can also download the CDM via Mozilla's manifest, checked against sha512.
  - Both pin `Host_11`, so a CDM interface change is a real risk.
- **Does Apple accept a non-browser host? Yes, in shipped code I didn't run.**
  - Kopuz v0.16.0 (Aug 2026) plays Apple Music this way, with a scraped web-player token.
  - ⚠ Kopuz caches **decrypted** audio on disk. Don't copy that.
- **No Kodi Apple Music addon exists.** The Breezyslasher one Perplexity cited is Apple TV+. Adjacent evidence: Apple's TV license server accepts Kodi's L3 CDM (~540p cap).

## 4. Existing Linux clients
| Client | How it plays |
|---|---|
| Cider 1 (AGPL) | castlabs Electron; developer token fetched from `api.cider.sh` |
| Cider 2 (closed source) | Electron on Linux/macOS; .NET + WebView2 on Windows |
| Sidra | castlabs Electron wrapping music.apple.com. **~381–440 MiB idle** [2nd, measured] |
| vibez (Go TUI) | headless Chrome over CDP running MusicKit JS; **own developer token** |
| amtui (Go TUI) | hidden Chromium on music.apple.com, driving `window.MusicKit` |
| **Kopuz (Rust)** | **its own CDM host, no browser** |

- WebKitGTK has no desktop Widevine [2nd]; vibez's WebKit backend only plays 30-second previews.
- **Kopuz is the only shipped native player I found.**

## 5. Tokens and ToS
- **Developer token** [src: https://developer.apple.com/documentation/applemusicapi/generating-developer-tokens]: requires an Apple Developer Program membership plus a MusicKit key. It's an ES256 JWT that expires after at most 15777000 s (6 months); an `origin` claim is optional.
- **How clients get one:** gamdl and Kopuz scrape Apple's web-player token; Cider 1 fetches one from its own server (whose, unclear); vibez signs its own.
- **Apple's license agreement** [src: §MusicKit, https://developer.apple.com/support/terms/apple-developer-program-license-agreement/]:
  - Content may be played "**only as rendered by the MusicKit APIs or MusicKit JS**".
  - No downloading or modifying content.
  - musickit.js itself forbids re-hosting it.
- **Music-User-Token through a localhost page: vibez already does it** [src]. It serves a localhost page that loads MusicKit from Apple's CDN, calls `authorize()`, and POSTs the token back. If you add an `origin` claim, it must cover localhost.

## 6. macOS
- `ApplicationMusicPlayer` is available on **macOS 14.0+**; `SystemMusicPlayer` is not on macOS [src: https://developer.apple.com/documentation/musickit/applicationmusicplayer].
- Needs an App ID with the MusicKit App Service, a matching bundle ID, and a Swift bridge.
- Distributing outside the App Store is [unverified].

## 7. Windows
- Apple's Windows app has **no public API**. Only the system media controls work; AMWin-RP has to scrape the window (https://github.com/PKBeam/AMWin-RP).
- WebView2 **has no Widevine**; the feature request has been open since 2024-09 (https://github.com/MicrosoftEdge/WebView2Feedback/issues/4828).
- WebView2 does have PlayReady, and MusicKit supports it. That it plays Apple Music is **inferred from Cider 2**, not verified.

## Ranked architectures
**B wins if compliance is the deciding factor; A wins if matching Spotifast's "no browser" design is.** librespot is unofficial in the same way A is.

| | Cost | Fragility | ToS/legal | Unverified |
|---|---|---|---|---|
| **B. Hidden Chrome sidecar running MusicKit JS** (own token; WebView2 on Windows) | One Chromium (≈400 MiB with full UI) | Medium: Chrome/Widevine/headless quirks; uses Apple's supported surface | **The only compliant Linux path**; $99/yr | Headless RAM; WebView2 PlayReady |
| **A. Native Rust CDM host** | Lowest (unmeasured) | High: private endpoints, `Host_11` pin, Google can revoke CDMs, Windows/macOS lack VMP | Breaks the developer agreement or impersonates Apple. Not legal advice | Kopuz not run; Win/Mac |
| **C. macOS ApplicationMusicPlayer** | Minimal | Low | Compliant | Signing outside the App Store |
| **D. Windows app via system media controls** | Minimal | Low | Fine | Play/pause/skip only: no search or queue |

Not viable: WebKitGTK; pywidevine or FairPlay "wrapper" (circumvention).
