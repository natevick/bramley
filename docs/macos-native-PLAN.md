# Native macOS build (MusicKit) plan

Decision (Nate, 2026-10-16): on macOS Bramley plays through Apple's MusicKit `ApplicationMusicPlayer` (macOS 14+). macOS does the DRM. The egui UI, CLI/MCP surface and `crates/api` browsing are unchanged. Linux keeps `bramley-engine` (QuickJS + Widevine) untouched.

## Architecture
- The playback protocol (`Command`, `Queue`, `Snapshot`, `PlayState`, `PlayerError`, `Repeat`, `Tokens`) plus `redact` and `user_agent` stay in `bramley-engine`. Its heavy half (QuickJS, Widevine, pulseaudio, symphonia, `Player`) sits behind the default feature `runtime`. The macOS build depends on the engine with `default-features = false`, which compiles none of that and publishes no new crate. `scripts/macos/check-no-engine-runtime.sh` (runs on Linux) fails if the macOS dependency tree reaches `pulseaudio`, `rquickjs` or `symphonia`, and `scripts/macos/bundle.sh` fails if the binary carries the `bramley-linked:engine` marker.
- The app's seam is the `Link` it already had: `(Sender<Command>, Receiver<Snapshot>)`. `launch()` in `crates/app/src/main.rs` picks the backend per target: engine on Linux/Windows, `bramley_macos_player::spawn` on macOS. `Playback` is untouched.
- `crates/macos-player`: one Swift file (`swift/MusicKitPlayer.swift`) compiled by `build.rs` with `xcrun swiftc -static` and linked into the bramley binary, so the .app is the MusicKit client. Five C-ABI functions, JSON strings both ways: authorization, tokens, command, snapshot, free. The Rust half (`wire.rs`) builds on every target so its tests run on Linux. It owns the command mapping, snapshot decoding and the library-id batching.
- `scripts/macos/bundle.sh` builds `Bramley.app` (env: `BRAMLEY_BUNDLE_ID`, `BRAMLEY_SIGN_IDENTITY`, optional `BRAMLEY_PROVISION_PROFILE`, whose entitlements it signs with). `bramley musickit-probe [--request]` prints the authorization status and a token-request outcome (lengths only).

## Sign-in and tokens
- `crates/app/src/musickit.rs` (`Session`) is the one place the app asks macOS. `DeveloperToken` and `Credentials` hold an optional session; when set, `get/peek/refetch/status/missing` and `load/save/clear/storage` answer from it. That covers `api_tokens`, the covers `Source`, `bramley warm` and the S29 cache scope (a digest of the user token, so the scope is the MusicKit user token).
- Tokens live in memory only (held up to an hour, dropped on a new sign-in and on sign-out). The developer-token fetch, its cache file and the keyring are never used on macOS; `save` refuses.
- `check_sign_in` reads `MusicAuthorization.currentStatus` (`Credentials::signed_in`): authorized is signed in, everything else signed out. The "Sign in" button (and `bramley sign-in`) runs `MusicAuthorization.request()` on a worker through the existing `checking` job, so the UI states are the ones the keyring check already drives. Denied and restricted show a notice that names System Settings.
- "Sign out" on macOS only forgets the in-app tokens; access is revoked in System Settings > Privacy & Security > Media & Apple Music.
- `bramley warm` runs through the same `Credentials` and works from the signed binary (see Background warming).

## Playback mapping
`SetQueue` (catalog and library songs, albums, playlists, with `start`), `Play`, `Pause`, `Next`, `Previous`, `Seek`, `Shuffle`, `Repeat`, `PlayLater`/`TopUp` (tail insert), `PlayNext`, `JumpTo` (refused if the entry at the index is no longer that id). Implemented, and the Swift type-checks, but no command or snapshot call has run yet: the player only spawns once MusicKit is authorized, which waits for the signed run. Library ids (`i.`, `l.`, `p.`; `pl.` is catalog) resolve with `MusicLibraryRequest.filter(matching:memberOf:)`, 100 per request, one request per batch; library albums and playlists go through `MusicLibraryRequest<Album|Playlist>`. For songs the start item is found by id, so a song MusicKit could not resolve never shifts playback onto its neighbour. The snapshot window starts at the current entry (row 0 is the playing one), like the engine's.

## Volume: recommendation
`ApplicationMusicPlayer` has no volume API. Hide the slider on macOS (done: the player bar skips it) and ignore `Command::Volume` (counted as applied so the app's waits settle). The system-volume alternative changes every app's output on the Mac, so it is not worth a hack. `bramley volume N` is accepted and does nothing on macOS.

## Unsigned result (Air, macOS 27.0.1, ad-hoc signature)
`{"authorization":"notDetermined","tokens":{"error":"MusicTokenRequestError: .permissionDenied"}}`. Nothing past the token request was attempted, and `--request` is not run unsigned (it could raise a prompt).

## The signed run (Nate, in Terminal on the Air's own screen)
The Apple Development certificate lives in the login keychain, which is locked to ssh, so the run has to start from his GUI session.
1. `cd ~/repos/bramley-macnative && scripts/macos/signed-check.sh` (no arguments).
2. It finds the "Apple Development" identity, builds and signs `Bramley.app` as `codes.vick.bramley`, then launches it through LaunchServices (`open -W`) so the consent prompt belongs to Bramley, not Terminal: once status-only, once `musickit-probe --request` (click Allow on the prompt) followed by the token request.
3. Result, statuses and lengths only, ending in a one-line `VERDICT:`: `~/bramley-spike/signed-check.out`. The signed bundle stays in `~/bramley-spike/signed/Bramley.app` for later re-probes over ssh. Success is `"authorization":"authorized"` and `developerBytes`/`userBytes` above zero.
4. If the verdict is NO TOKENS (consent not asked, or tokens refused), macOS likely needs a profile. Create one by letting Xcode manage it (stub macOS App project, bundle id `codes.vick.bramley`, automatic signing, MusicKit capability, build once), then re-run; the script finds it under `~/Library/Developer/Xcode/UserData/Provisioning Profiles`, or set `BRAMLEY_PROVISION_PROFILE`.

## Silent tests
No test calls `play()`. Swift is exercised only through its JSON replies. For the later signed end-to-end run, proposal: mute the default output with `osascript -e 'set volume output muted true'`, run, and restore the previous mute state in a trap. The alternative is a virtual output device (BlackHole), which needs an install. Nate decides.

## Open questions (the rest only a signed run answers)
- The Swift command and snapshot path has never executed (queue building, library requests, `JumpTo`, the snapshot window).
- Does `DefaultMusicTokenProvider` work with only a MusicKit App Service App ID, or does macOS also need an embedded provisioning profile?
- Do MusicKit's library ids equal the API's `i.`/`l.`/`p.` ids, and does `itemId` equal the queue ids the app compares against?
- Does the consent prompt appear when the probe is launched via `open`, and does `MusicAuthorization.request()` need the main run loop (the probe pumps it when called on the main thread)?
- Do the three-state `shuffleMode`/`repeatMode` reads track what the UI toggles?

## Cache on macOS (measured on the Air, 2026-10-21, signed app)
The cache is not broken, and the scope key needed no change. The user token is identical across launches (12 token fetches over 6 launches, `same_as_prev_launch=true` on every relaunch, no fresh fetch, no failure), so the S29 scope is stable. Evidence, from an instrumented build that was not kept:
- A relaunch against the same cache dir hit on every list and every album already seen (`apiHits` 7, `apiMisses` 0); against Nate's real `~/.cache/bramley`, written by his earlier session, `albums` and `songs` hit (`apiHits` 3, `apiMisses` 0).
- The token path is not a per-request cost: `MusicAuthorization` status answers in 0 ms (159 of 165 calls; the other 6 took 6 to 7 ms) and `api_tokens` took 0 to 9 ms.
- Library lists, library album tracks and art all go through the same `Session` and `Store` code as Linux and all produce hits.
- Over ssh, `bramley now` reaches the app: the instance socket is `$TMPDIR/bramley`, and with `TMPDIR` unset (the ssh case) the client still finds the GUI session's directory. It printed nothing earlier because the app was not running and the "not running" message goes to stderr. An empty-but-set `TMPDIR=` does break it (the slot becomes a relative path); nothing in launchd or ssh sets that.
Home (the chart) is cached too: a cold launch was 1 miss and 50 art misses, every relaunch 1 hit and 50 art hits. Playlist tracks were not measured; they use the same `related()` path as album tracks. `/v1/me/recent/played` and catalog search are outside the cache allowlist on every platform.
Open lead, unproven: with every cover a cache hit, the 50 Home cover textures still took 4.8 to 7.4 s to all appear, while the window was behind another app and painted only about two frames per probe. That looks like occlusion-throttled repainting rather than the cache, and needs the same count taken with the window in front.
What is slow on macOS besides that is the first visit, and only that: a cold launch took about 2.4 to 2.6 s to show the album list and 0.4 to 0.5 s per unseen album, against at most 0.27 s from cache (the probe's own floor: a 150 ms sleep plus the CLI round trip and polling, so the true cached time is lower), and Linux hides it with the warm timer. macOS had no scheduler, which is the gap the next section closes.

## Background warming on macOS
`packaging/macos/codes.vick.bramley.warm.plist` is copied into `Bramley.app/Contents/Resources` by `bundle.sh`. It runs `Bramley.app/Contents/MacOS/bramley warm` (the signed binary, so MusicKit tokens work) every 300 s, at background priority with low-priority I/O, and logs to `~/Library/Logs/bramley-warm.log`. Run `scripts/macos/enable-warm.sh [/path/to/Bramley.app]` (default `/Applications/Bramley.app`) to fill in the app path, install it into `~/Library/LaunchAgents` and `launchctl bootstrap gui/$(id -u)` it; `launchctl bootout gui/$(id -u)/codes.vick.bramley.warm` stops it. Budget, daily refresh, backoff and the app-running check are the Linux ones, because it is the same `bramley warm`. `scripts/macos/enable-warm.test.sh` checks the installed plist and the bootstrap call without macOS.
Proof run on the Air, signed app, three ways: with the app closed through `open -W`, `requests=59 skipped=0 stopped=budget`; with the app open, `requests=0 skipped=0 stopped=app-running`; and as a one-shot `launchctl submit` job running the binary directly (the agent's path), `requests=59 skipped=0 stopped=budget`.

## Profile, WWDR and icon
`scripts/macos/make-profile.sh` (env `BRAMLEY_TEAM_ID`, optional `BRAMLEY_BUNDLE_ID`) generates the stub app with xcodegen, adds the push entitlement and builds it with `xcodebuild -allowProvisioningUpdates -allowProvisioningDeviceRegistration`, so Xcode creates the profile that carries MusicKit; it prints the profile path to give `BRAMLEY_PROVISION_PROFILE`. Run it in Terminal on the Mac's own screen: the signing key is in the login keychain.
The Apple Development certificate must chain to the Apple Worldwide Developer Relations Certification Authority G3 intermediate. Without it in the keychain the identity does not validate and neither Xcode nor `bundle.sh` can sign. (Over ssh `security find-identity -v -p codesigning` also reports 0 valid identities, but that is the locked login keychain, not G3.) Install the G3 certificate from apple.com/certificateauthority.
The app icon is `packaging/macos/AppIcon.icns`, committed so `bundle.sh` needs no renderer; `scripts/macos/make-icon.sh` rebuilds it from `packaging/icons/bramley.svg` with `sips` and `iconutil` (ImageMagick's SVG renderer draws the apple black). The Info.plist sets `CFBundleIconFile`.
