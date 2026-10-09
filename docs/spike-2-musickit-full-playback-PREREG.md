# Spike 2: own developer token plays FULL Apple Music songs (pre-registered 2026-10-09)

**Question.** Does a developer token signed with Nate's own MusicKit key, plus a Music-User-Token from a one-time loopback sign-in, play full-length songs through unmodified MusicKit JS? This is the premise everything else rests on.

**Run in real Chrome first** (headless, fresh profile, spike-1 harness). If it fails there, it is Apple refusing the token, not our runtime.

**Setup**
- **Developer token:** an ES256 JWT minted through the broker (`oss` profile). It has `kid` = key ID, `iss` = team ID, a 180-day expiry, and no `origin` claim.
- **User token:** from a localhost page that loads musickit.js from Apple's CDN, calls `authorize()`, and posts the token back to a 127.0.0.1 listener.
- **Storage:** both tokens are kept at `~/.config/bramley/`, mode 0600, never committed, never logged.
- **Into Chrome:** the user token is injected through MusicKit's public `musicUserToken` setter, or its documented storage. MusicKit is never patched.

**Pass: all of these, or FAIL**
1. `music.isAuthorized === true` and a non-empty storefront.
2. A catalog song longer than 60 s plays to `currentPlaybackTime` > 35 s and keeps advancing. A preview stops at 30 s.
3. EME: a `com.widevine.alpha` key reaches `usable`. Record the encryption scheme: cbcs or cenc.
4. A PipeWire output stream from Chrome is linked to a temporary null sink. The default sink is unchanged.

**Did-it-run and sabotage**
- **Sabotage A:** run without the user token. MusicKit plays a 30 s preview, so check 2 must FAIL. This proves the preview guard works.
- **Sabotage B:** a corrupted developer token. Configure or authorize must fail, and the verdict must be VOID or FAIL.

**Also record**
- The API surface MusicKit touches during authorize, play, pause, seek and skip, using logging getters through `addScriptToEvaluateOnNewDocument` (spike only).
- The asset flavor and codec MusicKit picked.
- musickit.js's ETag.

**Named false positives**
- A preview mistaken for a full song. Checks 2 and A guard this.
- An authorized state left over from a reused profile. Every run uses a fresh profile.
- Audio playing in the authorize popup rather than the player.
