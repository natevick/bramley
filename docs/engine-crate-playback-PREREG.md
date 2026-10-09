# Engine crate playback checks (pre-registered 2026-10-09)

Written before the first run of `scripts/measure-playback.sh` against `crates/engine`. The thresholds below do not move after seeing data.

## 1. Steady playback (port of spike 5)

`scripts/measure-playback.sh <name>`: Angel One Widevine HLS through hls.js 1.7.3, looping, into a private PipeWire null sink. PSS summed over the process tree, 15 s warm-up, then 60 s of samples every 2 s.

- **PASS:** PSS median ≤ 105 MiB and every did-it-run check passes. 105–125 is MARGINAL; above 125 is FAIL.
- **Did-it-run (any failure means VOID):**
  - JS saw a `usable` key, and hls.js loaded a key.
  - JS `currentTime` passed 30 s.
  - More than 30 s was decoded from decrypted samples.
  - Our PID's PipeWire stream was linked to the null sink.
  - The process tree was sampled and did not exit early.
  - The default sink was unchanged.
- **Sabotage:** `--lic https://cwip-shaka-proxy.appspot.com/no_auth_SABOTAGED` must read VOID.

## 2. Pause/seek scenario

`scripts/measure-playback.sh --scenario <name>`: same stream, no loop, hls.js `backBufferLength` 10 s. The run plays to 15 s, pauses for 20 s, seeks to 40 s, plays, takes a snapshot 5 s after resuming, and plays to the end. PSS is sampled from 5 s to 120 s; the 120 s is the sampling window, and the content is 60 s long.

At each step the engine-play binary records, on the same thread and in the same instant:
- JS `currentTime`, `paused`, `seeking` and `ended`;
- the native output position, which is the media PTS of the last PCM handed to the audio output, carried from the demuxed frame timestamps rather than derived from `currentTime`;
- native decoded seconds;
- wall time.

**Did-it-run (any failure means VOID):**
- All five steps recorded: `pause-at-15`, `after-20s-pause`, `seeked-to-40`, `5s-after-resume`, `ended`.
- `seeking`, `seeked` and `ended` events were seen.
- The stream was linked to the null sink, and the default sink was unchanged.
- More than 20 s was decoded from decrypted samples.
- PSS was sampled and the process did not exit early.

**PASS requires all of the following (any failure means FAIL):**
1. At `pause-at-15`, `5s-after-resume` and `ended`, |JS `currentTime` − native output position| ≤ 0.5 s.
2. A `pause` event fired. Between `pause-at-15` and `after-20s-pause`:
   - at least 20 s of wall time passed;
   - JS `currentTime` moved by ≤ 0.1 s;
   - the native output position moved by ≤ 0.25 s, which is at most one in-flight output request;
   - JS reports `paused`.
3. At `seeked-to-40`, |JS `currentTime` − 40| ≤ 0.5 s, and `seeking` is false.
4. Between `seeked-to-40` and `5s-after-resume`, |Δ`currentTime` − Δwall| ≤ 0.5 s.
5. At `ended`:
   - |JS `currentTime` − buffered end| ≤ 0.5 s;
   - the decoded seconds added between `seeked-to-40` and `ended` agree with the `currentTime` advance over the same span within 0.5 s.
6. PSS growth (the maximum sample minus the first sample) ≤ 5 MiB.

**False positives named in advance:**
- A frozen `currentTime` alone proves nothing if audio kept flowing. That is why check 2 also bounds the native output position.
- A matching output position at `seeked-to-40` is meaningless, because nothing has been rendered while paused. That is why check 1 skips it.

**Sabotage:** `--scenario-skip-pause` records the steps but never calls `pause()`. It must read FAIL, because check 2 trips. It must not read VOID.

## Addendum, 2026-10-09, after the first scenario run (`scen1`)

`scen1` read PASS, but `SourceBuffer.remove()` never ran. hls.js fired `BACK_BUFFER_REACHED` seven times, but it refuses to flush the back buffer once the track's SourceBuffer is in the ended state. The 60 s stream was fully buffered by about 15 s, with the default `maxBufferLength` of 30 s.

The scenario now also passes `maxBufferLength` 10 s, so the track is still loading while the back buffer fills. That flag also sets `maxBufferSize` to 0, because hls.js takes the larger of `maxBufferLength` and 8 × `maxBufferSize` / bitrate. At 140 kbps the default size would allow about an hour of buffer. `scen2` showed this: it set only the length, and remove() still did not run (VOID under the new check). A new did-it-run check requires at least one `sb_remove` op. No threshold moved. `scen1` stays on record as a PASS that did not exercise eviction.

## Addendum, 2026-10-09: ten back-to-back tracks (review finding M6)

Written before the first run. `cargo test -p engine --test playback ten_back_to_back -- --ignored` runs `engine-play --tracks 10 --track-seconds 20 --no-loop` (12 s in run 1; see the amendment) into a private null sink. Each "track" is a fresh `Hls` instance on the same `<audio>` element: a new MediaSource and `src`, new SourceBuffers and new MediaKeys. The previous instance is destroyed while playing, at `currentTime` ≥ the track length. At each track boundary the binary runs the JS GC and reads its own PSS from `/proc/self/smaps_rollup` (one process).

- **Did-it-run (any failure means VOID):**
  - 10 track boundaries were recorded.
  - Every track loaded at least one new key (`KEY_LOADED` count rose).
  - Every track decoded at least 9 s from decrypted samples.
  - `cdm_create`, `ms_create` and `sb_add` each ran at least 10 times.
- **PASS:** PSS at the end of track 10 minus PSS at the end of track 1 ≤ 8 MiB. Otherwise FAIL.
- **Sabotage:** the same run on 344e38e (the head before the M6 fix), with only the binary's track mode and a `run_gc` seam ported, is expected to read FAIL. If it reads PASS, this check cannot see M6 and is reported as such.

**Amendment after run 1 (VOID):** with 12 s tracks every track decrypted only 5.5–5.7 s, because Angel One opens with about 6.5 s of clear audio. Run 1 is VOID under the 9 s check above and stays on record. The track length moves to 20 s. No threshold moved.
