# Engine crate playback results (2026-10-09)

The pre-registration is `engine-crate-playback-PREREG.md`. The binary under test is `crates/engine`'s `engine-play` release build, driven by `scripts/measure-playback.sh`. Values are MiB, PSS summed over the process tree. There is one process.

## Steady playback (port of spike 5): **PASS**

| Run | PSS med (max) | RSS | USS | JS max `currentTime` | Decrypted s | Clear s | Verdict |
|---|---|---|---|---|---|---|---|
| run1 | **97.6** (97.6) | 101.3 | 97.5 | 60.02 | 60.6 | 16.0 | PASS |
| run2 | **97.7** (97.7) | 101.4 | 97.6 | 60.02 | 60.5 | 16.0 | PASS |
| sabotage-license | 89.6 | 93.3 | 89.5 | 7.92 | 0 | 8.0 | VOID |

- Every did-it-run check passed in both real runs.
  - There were 13 `KEY_LOADED` events, 0 decrypt failures and 0 decode errors, and one loop at 60 s.
  - The stream was linked to `engine-measure-null`, and the default sink was unchanged.
- In the sabotage run the license request returned 404, so hls.js raised a fatal `keySystemLicenseRequestFailed`. Only the 8 s clear lead-in played.
- This is +0.4–1.1 MiB against spike 5's 96.6–97.2.

## Pause/seek scenario: **PASS** (`scen3`)

Steps, with JS `currentTime` / native output position in seconds:

| Step | Wall s | JS `currentTime` | Native output | Paused |
|---|---|---|---|---|
| pause-at-15 | 15.42 | 15.0053 | 15.0051 | no |
| after-20s-pause | 35.42 | 15.0053 | 15.0051 | yes |
| seeked-to-40 | 35.55 | 40.0000 | (nothing rendered while paused) | yes |
| 5s-after-resume | 40.55 | 44.9787 | 44.9786 | no |
| ended | 55.65 | 60.0213 | 60.0210 | yes |

**What was checked:**
- **Agreement:** JS `currentTime` and the native output position differ by at most 0.0003 s at every step that rendered audio.
  - This check has a named false positive: both sides are updated in the same write, so it cannot see output latency or a lost tail. It passed while the last ~200 ms of every track was being cut (review finding M5, since fixed).
- **Pause:** it held 20.00 s with 0.0 drift on both measures.
- **Seek:** it landed exactly on 40.
- **Resume:** the advance matched wall time within 0.025 s.
- **Decoded audio:** decoded seconds from 40 to the end matched the `currentTime` advance within 0.000001 s.
- **Memory:** PSS was 96.30 → 96.98 at most, a growth of 0.68 MiB (bar: 5).
- **Events:** `pause, seeking, seeked, play, waiting, canplay, playing, canplaythrough, durationchange, pause, ended`.
  - The seek to 40 was outside the buffered range.
  - `seeked` fired once 40 was buffered. `play` then waited for more data.
- **Eviction:** hls.js called `SourceBuffer.remove()` twice. The buffer ended at [36.0, 60.0], 384 KB.

**Sabotage (`--scenario-skip-pause`): FAIL, as required.**
- `currentTime` and the output position each drifted 19.97 s across the "pause", so check 2 failed.
- Check 5 also failed: decoded vs advance was 1.5 s.

**Superseded runs, kept on record:**
- `scen1` read PASS without ever calling `remove()`. hls.js won't flush the back buffer of an ended SourceBuffer, and the 60 s stream was fully buffered.
- `scen2` capped `maxBufferLength` alone, and hls.js still buffered everything, so it read VOID under the new check.
- See the addendum in the pre-registration.

## Re-run after the review fixes (2026-10-09)

Same scripts and thresholds, on the branch with the review findings fixed.

| Run | Verdict | Key numbers |
|---|---|---|
| `fix-run1` | **PASS** | PSS median 98.2 MiB; every did-it-run check passed |
| `fix-sabotage-license` | VOID, as required | 90.4 MiB; no usable key, nothing decrypted |
| `fix-scen` | **PASS** | max \|currentTime − output\| 0.0003 s; pause drift 0.0; seek 40.000; PSS growth 0.35 MiB; drain acked 1, timeouts 0 |
| `fix-scen-skip-pause` | FAIL, as required | 19.97 s drift across the "pause"; checks 2 and 5 failed |
| ten tracks (PREREG addendum) | **PASS** | PSS 102.4 → 99.3 MiB over ten fresh loads; live CDMs 0 at the end |

The ten-track check cannot see M6 at its 8 MiB bar: on 344e38e it also passed (+2.7 MiB), because each leaked Widevine instance costs about 0.3 MiB. The unit test on live native object counts is the evidence for M6.

**Left as they are:**
- m4 (process-global `g_lib`): the engine runs one Engine on one thread with one CDM path, and the Widevine module itself is initialised once per process.
- m8 (`status()` holds the borrow across `statusJson`): still safe because `statusJson` calls no ops; the new live-object counts are computed in Rust, not through an op.

## Caveats
- **The content is 60 s.** "Play to the end" finishes about 55 s into the 120 s window, and PSS flatness is shown over that run only. Engine-side quota eviction (4 MiB) never triggers on a 1 MB stream. It is covered only by unit tests, including a 400 s synthetic timeline.
- **`currentTime` counts frames written to the PulseAudio server, so it leads the audible position by up to the 200 ms server buffer.** This is inherited from spike 5. After a seek while playing, it jumps about 0.2 s as the emptied server buffer refills (seen in the sabotage run: 40.2).
- **This is a stand-in for Apple.** No musickit.js, Apple fork of hls.js, or Apple license server was involved.
