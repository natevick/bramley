# Spike 2 results: own developer token plays FULL Apple Music songs (2026-10-09)

**Verdict: PASS.** - A developer token signed with Nate's own MusicKit key, plus a Music-User-Token from the loopback sign-in, played Bohemian Rhapsody (355 s) at full length through unmodified musickit.js.
- The player was headless Chrome 154 with a fresh profile and an honest headless UA. Apple did not refuse based on origin (`http://127.0.0.1`) or UA.

## Checks (run `s2-real`)

| Check | Result | Evidence |
|---|---|---|
| 1. Authorized with a storefront | ✓ | `isAuthorized` true, storefront `us`, `previewOnly` false |
| 2. Full song past 35 s | ✓ | Duration 355,155 ms. `currentPlaybackTime` climbed from 1 to 46 s over 46 s of wall time and was still advancing; a preview stops at 30. |
| 3. Widevine key usable | ✓ | `com.widevine.alpha` granted, 2 sessions, statuses `usable`. First usable key at 3.5 s; first audio at 4.2 s from cold launch. |
| 4. Audio to the null sink only | ✓ | Chrome's PipeWire stream was linked only to `spike2-null`. The default sink was unchanged. |

**Controls, all through MusicKit's public API:**
- Pause froze playback at 46.36 s for 4 s.
- `seekToTime(120)` landed on 120.
- Resume advanced to 126.
- `skipToNextItem` switched to Don't Stop Me Now (6781079518), which was playing at 9 s.

## Sabotage

| Run | Verdict | What happened |
|---|---|---|
| A: no user token | **FAIL** (as required) | `isAuthorized` was false and MusicKit played the **preview**. `currentPlaybackTime` stopped at exactly 30.0 s and never advanced, so check 2 failed. The preview guard works. |
| B: corrupted developer token | **FAIL** (as required) | `setQueue` failed with `REQUEST_ERROR` on the catalog lookup and nothing played. `isAuthorized` still read true, because it only reflects that a token is stored (see caveats). |

## What MusicKit picked
- **Flavor:** `28:ctrp256`, AAC-LC 256 kbps (`mp4a.40.2`). No hls.js: MusicKit fetched one fMP4 asset by HTTP byte range from `aod-ssl.itunes.apple.com` and appended it through MSE itself.
- **Encryption:** the scheme is **cenc** (`schm`), not cbcs. The original format is `mp4a`, and the PSSH is Widevine's.
- **License host:** `play.itunes.apple.com`, path `/WebObjects/MZPlay.woa/wa/acquireWebPlaybackLicense`. The server certificate comes from `.../wa/widevineCert` through `MediaKeys.setServerCertificate()`.
- **musickit.js:** ETag `"a62a15037b8a40a25429bfe93bfdb738"`, version `3.2526.0-prerelease.x`, the same file spike 4 used.
- **User token location:** MusicKit reads it from `localStorage["music.<team id from the developer token's iss, lowercased>.media-user-token"]`. The harness seeds that key before configure; MusicKit is never patched.

## Memory (reference only)
Chrome used 586 MiB PSS median while playing, untuned. This is the rejected sidecar path.

## API contract
`docs/musickit-api-contract-chrome.md` lists 104 web API members Apple's code called, 31 of which the spike-4 seed already had. Its "observed playback facts" section lists four things our runtime does not do yet:
- `setServerCertificate()`
- `persistentState: "required"`
- a Widevine config that also asks for video
- `SourceBuffer.remove()`, `timestampOffset` and `appendWindowStart`

## Caveats
- The passing run was instrumented; checks 1, 2 and 4 don't depend on it.
- **cbcs not observed:** cenc is what Apple served to Chrome on Linux. cbcs (tested in the engine) wasn't seen here.

## Token hygiene
Neither token is printed, logged or committed. Both are read from `~/.config/bramley/` at run time, and every result file passes through a redaction step. A grep of every output and the staged diff for both token values found no match. The only `eyJ` hits are the redaction regex and this sentence.

## Re-run
```
cd /home/norm/projects/Personal/bramley
spikes/02-musickit-surface/harness/run.sh s2-real
spikes/02-musickit-surface/harness/run.sh s2-sabotage-no-user --variant no-user
spikes/02-musickit-surface/harness/run.sh s2-sabotage-bad-dev --variant bad-dev
python3 -I spikes/02-musickit-surface/harness/gen_contract.py s2-real
```
