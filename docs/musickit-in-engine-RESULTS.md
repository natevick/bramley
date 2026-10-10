# MusicKit JS in the Bramley engine: results (2026-10-10)

The pre-registration is `musickit-in-engine-PREREG.md`. The binary under test is `bramley-musickit` (release, `--features sabotage,diagnostics`), driven by `scripts/measure-musickit.sh` and judged by `scripts/musickit/verdict.py`. The verdict JSONs are in `docs/results/musickit-in-engine/`.

**Verdict: PASS.** Apple's unmodified musickit.js played Bohemian Rhapsody in full inside the engine, with no browser, in two runs.

## Checks (run1 / run2)

| # | Check | Result |
|---|---|---|
| 1 | Authorized, storefront `us` | `isAuthorized` true, `us`; `/v1/me/storefront` 200 |
| 2 | Past 35 s, still advancing; > 35 s decrypted | JS time 79 s, +10 s over the last 10 s; 80.1 / 80.2 s decoded from decrypted samples, 0 s clear |
| 3 | Usable key | `usable`; license host `play.itunes.apple.com`; scheme `cenc`; codec `mp4a.40.2` at 44.1 kHz (native init segment) |
| 4 | Null sink only | stream linked only to `musickit-measure-null`; default sink unchanged |
| 5 | PSS median, 60 s steady | **97.7 / 100.3 MiB** (max 99.4 / 100.6): PASS against ≤ 112 |
| 6 | Pause, seek 120, resume, skip, each within 0.5 s | pause held 78.62→78.62; seek 120.000; resume first played at 120.20, 125.19 at +5 s; skip first played at 0.20, 7.6–7.7 at +8 s; clean exit |

Check 6 uses the engine's native `currentTime`, because MusicKit's `currentPlaybackTime` is whole seconds. Native time leads the audible position by up to 200 ms (PulseAudio buffer), so the resume and skip margins are about 0.3 s, not 0.5.

## Sabotage

| Run | Verdict | What happened |
|---|---|---|
| A: no user token | FAIL | MusicKit came up unauthorized; the bin stopped with "sign-in needed" and nothing played |
| B: corrupted developer token | FAIL | `/v1/me/storefront` 401; "sign-in needed"; nothing played |
| C: musickit.js truncated in memory | VOID | 307,400 of 614,800 bytes evaluated, SyntaxError, `MusicKit` undefined |

Sabotage A did **not** exercise the PREREG's 30 s preview guard. The bin now refuses to run signed-out, and the engine cannot play MusicKit's progressive-https preview anyway. The "preview counted as a full song" false positive is instead guarded by check 2's requirement of more than 35 s decoded from **decrypted** samples: a clear preview decrypts 0 s.

## Apple's script and endpoints
- `https://js-cdn.music.apple.com/musickit/v3/musickit.js`, sha256 `21908b72bdcdeea3ec91fa40b105d0b9a2ba07c2a3fe32ea77edcd26e735012c`, 614,800 bytes. The fetched and evaluated hashes are equal, and it was the only script loaded.
- Every Apple call came from MusicKit, through the engine's fetch, with UA `Bramley/0.1.0 (linux)` and origin `app://bramley`. Every response was 2xx (run1):
  - `api.music.apple.com`: catalog songs, me/account, me/storefront
  - `play.itunes.apple.com`: widevineCert ×1, webPlayback ×2, acquireWebPlaybackLicense ×2
  - `universal-activity-service.itunes.apple.com/play`
- **Regression checks:** `measure-playback.sh` PASS at 97.6 MiB, and the ten-track test PASS (95.4–99.6 MiB). The HLS, cbcs and Apple e2e tests pass.

## Caveats
- **User token.** It is seeded into MusicKit's undocumented storage key, per the 2026-10-10 interpretation log. The key may change; if MusicKit is not authorized afterwards, the bin exits "sign-in needed".
- **Bitrate.** The player sets `music.bitrate = PlaybackBitrate.HIGH`. Without it, MusicKit picks 64 kbps HE-AAC, and the engine decodes only its 22.05 kHz core.
- **Video capability.** The DRM gate is answered as the 10-10 log allows: avc1.42E01E at `SW_SECURE_CRYPTO` only. Video SourceBuffers throw.
- **Error dialog.** MusicKit's error dialog throws (`focus` of null), because `innerHTML` is a no-op in the engine. That hides `mediaPlaybackError` from MusicKit's own UI.
- **One unexplained run.** One early exploratory run never called `session.update()` after a 200 license response. It has not recurred in any run since.
- **Diagnostics disclosure.** In two exploratory runs, MusicKit's debug logging wrote a license payload to scratch logs. They were deleted, and those flags are banned from now on.
