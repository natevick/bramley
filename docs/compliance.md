# Compliance guardrails

This project plays Apple Music on Linux and Windows by running Apple's MusicKit JS, unmodified, inside our own small JavaScript runtime. That runtime provides natively implemented media and DRM host objects (media element, Media Source Extensions, Encrypted Media Extensions) backed by the Widevine CDM. Nate chose this direction on 2026-10-09. It is a grey area: the terms below are written with web pages in mind. These rules are what keep it defensible. Breaking any of them takes the project outside the agreement. This is not legal advice.

## Rules

1. **musickit.js is fetched from Apple's CDN at runtime, byte for byte.** It is never committed, bundled, re-hosted or patched. Standard HTTP caching that honours Apple's cache headers is allowed. A modified copy on disk is not. `vendor-cache/` directories are gitignored and exist only for spike analysis.
2. **Shims are host objects, the way a browser's built-ins are.** They implement web platform interfaces (HTMLMediaElement, MediaSource, SourceBuffer, MediaKeys, fetch, Storage). They never reach into MusicKit's internals, monkey-patch its functions, or rewrite its source.
3. **App logic calls only MusicKit JS's public, documented API.** It lives in our own separate script. Nothing is concatenated with or injected into musickit.js.
4. **Never call Apple's undocumented playback or license endpoints ourselves.** Only MusicKit JS talks to them, through our fetch host object. Our Rust code may call the documented Apple Music API (api.music.apple.com) for catalog and library browsing.
5. **Never spoof Apple's Origin or User-Agent** to look like music.apple.com or a real browser. If Apple gates playback on either, stop and escalate to Nate. Do not work around it.
6. **Decrypted audio never touches disk.** There is no decrypted cache and no export, recording or "download" feature.
7. **Our own developer token, signed with Nate's MusicKit key.** Never use a token scraped from Apple's web player. The `.p8` private key stays in 1Password (broker `personal` profile). Only the minted JWT (maximum 6 months) lives in local config.
8. **Full songs, user-initiated, with standard controls** (play, pause, skip), as section D requires. No paid tier, ads or data harvesting.
9. **Open question for Nate:** an EQ on the playback path is arguably "modifying" content. Visualisers are fine. Do not ship an EQ without his call.
10. **Follow the Apple Music Identity Guidelines** for the app name, icon and any Apple Music branding. Read them before naming anything.

## Source text (verbatim, fetched 2026-10-09)

Apple Developer Program License Agreement, https://developer.apple.com/support/terms/apple-developer-program-license-agreement/

Definition of Application. It is Apple-platform-only, which is why MusicKit JS is the only sanctioned route on Linux and Windows:

> “ Application ” means one or more software programs (including extensions, media, and Libraries that are enclosed in a single software bundle) developed by You in compliance with the Documentation and the Program Requirements, for distribution under Your own trademark or brand, and for specific use with an Apple-branded product running iOS, iPadOS, macOS, tvOS, visionOS, or watchOS as applicable, including bug fixes, updates, upgrades, modifications, enhancements, supplements to, revisions, new releases and new versions of such software programs.

Definitions:

> “ MusicKit APIs ” means the set of APIs that enable Apple Music users to access their subscription through Your Application or as otherwise permitted by Apple in the Documentation. “ MusicKit Content ” means music, video, and/or graphical content rendered through the MusicKit APIs. “ MusicKit JS ” means the JavaScript library that enables Apple Music users to access their subscription through Your Applications, websites, or web applications.

Attachment / section on MusicKit:

> D. MusicKit You agree not to call the MusicKit APIs or use MusicKit JS (or otherwise attempt to gain information through the MusicKit APIs or MusicKit JS) for purposes unrelated to facilitating access to Your end users’ Apple Music subscriptions. If You access the MusicKit APIs or MusicKit JS, then You must follow the Apple Music Identity Guidelines. You agree not to require payment for or indirectly monetize access to the Apple Music service (e.g. in-app purchase, advertising, requesting user info) through Your use of the MusicKit APIs, MusicKit JS, or otherwise in any way. In addition: If You choose to offer music playback through the MusicKit APIs or MusicKit JS, full songs must be enabled for playback, and users must initiate playback and be able to navigate playback using standard media controls such as “play,” “pause,” and “skip”, and You agree to not misrepresent the functionality of these controls; You may not, and You may not permit Your end users to, download, upload, or modify any MusicKit Content and MusicKit Content cannot be synchronized with any other content, unless otherwise permitted by Apple in the Documentation; You may play MusicKit Content only as rendered by the MusicKit APIs or MusicKit JS and only as permitted in the Documentation (e.g., album art and music-related text from the MusicKit API may not be used separately from music playback or managing playlists); Metadata from users (such as playlists and favorites) may be used only to provide a service or function that is clearly disclosed to end users and that is directly relevant to the use of Your Application, website, or web application, as determined in Apple’s sole discretion; and You may use MusicKit JS only as a stand-alone library in Your Application, website, or web application and only as permitted in the Documentation (e.g., You agree not to recombine MusicKit JS with any other JavaScript code or separately download and re-host it).

Header of `https://js-cdn.music.apple.com/musickit/v3/musickit.js` (ETag `"a62a15037b8a40a25429bfe93bfdb738"`, Last-Modified 2025-06-26):

```
/**
 * IMPORTANT NOTE:
 *
 * This file is licensed only for the use of Apple developers in providing MusicKit Web Services,
 * and is subject to the Apple Media Services Terms and Conditions and the Apple Developer Program
 * License Agreement. You may not copy, modify, re-host, or create derivative works of this file or the
 * accompanying Documentation, or any part thereof, including any updates, without Apple's written consent.
 *
 * ACKNOWLEDGEMENTS:
 * https://js-cdn.music.apple.com/musickit/v1/acknowledgements.txt
 */
```

## Interpretation log

- **2026-10-09, runtime JS layer (Norm's reading, approved by Nate the same day: "Good call").** The web interfaces (media element, MSE, EME, fetch, events) may be written partly in JavaScript on top of native Rust ops, the way Deno implements web APIs and the way browsers implement some built-ins. This counts as host objects under rule 2 only if all of the following hold:
  - The layer is the runtime's own bootstrap, loaded before any page script, in its own script unit.
  - It never shares a file or source string with musickit.js.
  - It never references MusicKit internals.
  - It behaves like the standard interface, not a MusicKit-specific patch.
  - Concatenating it with musickit.js or reaching into MusicKit's objects is still forbidden.
- **2026-10-10, audio-only answer to MusicKit's DRM gate (approved by Nate).** musickit.js enables full-song DRM only when two things hold: `document.createElement("video").canPlayType('video/mp4;codecs="avc1.42E01E"')` is truthy, and Widevine grants a combined config of avc1 video at `SW_SECURE_CRYPTO` plus mp4a audio. This is confirmed in musickit.js sha256 `21908b72…012c`, around offset 116167. Bramley answers as follows:
  - **EME:** grant avc1 at `SW_SECURE_CRYPTO` only. That robustness level promises decryption, which the CDM performs for any track. Higher robustness levels are not granted.
  - **canPlayType:** return `"maybe"` for avc1 in mp4, the way a browser's `<audio>` element answers for video containers. It plays the sound and drops the picture.
  - **Video SourceBuffers:** `addSourceBuffer` for video throws NotSupportedError in v1. Accept-and-discard comes later.
  - **Scope:** songs carry no video track, so nothing is faked for music. Music videos fail loudly in v1.
