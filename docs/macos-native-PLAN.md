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
- `bramley warm` is wired through the same `Credentials`, so it should work from the signed binary (unexecuted); nothing schedules it on macOS (the timer is systemd).

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

## Open questions (only a signed run answers them)
- The Swift command and snapshot path has never executed (queue building, library requests, `JumpTo`, the snapshot window).
- Does `DefaultMusicTokenProvider` work with only a MusicKit App Service App ID, or does macOS also need an embedded provisioning profile?
- Do MusicKit's library ids equal the API's `i.`/`l.`/`p.` ids, and does `itemId` equal the queue ids the app compares against?
- Does the consent prompt appear when the probe is launched via `open`, and does `MusicAuthorization.request()` need the main run loop (the probe pumps it when called on the main thread)?
- Do the three-state `shuffleMode`/`repeatMode` reads track what the UI toggles?
- Is the user token stable across launches? If MusicKit rotates it, the S29 cache scope changes and the cache starts cold.
