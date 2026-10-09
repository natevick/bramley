# Spike 1 results: Chrome playback sidecar memory (2026-10-09)

**Verdict: FAIL.** The best tuned config (T2) has a median **PSS of 400 MiB** across the whole process tree while playing. The pre-registered FAIL line is >200 MiB. Pausing does not help (422 MiB). Chrome 154.0.8037.97, `--headless=new`.

## Results
Every run used 15 s of warm-up, then 31 samples over 60 s. All did-it-run checks passed except for the sabotage runs. Values are medians in MiB.

| Config | PSS | RSS | USS | Procs |
|---|---|---|---|---|
| S (stock) | 570.3 | 1404.9 | 383.4 | 14 |
| F4a `OptimizationHints` off (kills a 102 MiB PassageEmbeddings ML service) | 491.1 | 1262.3 | 304.1 | 12 |
| F13 extensions + omnibox WebUI off | 545.3 | 1264.8 | 378.9 | 13 |
| F5 audio in-process / F5b network in-process / F6a renderer limit 1 | ~566–577 | | | 13 each |
| T1 (T2 + `--in-process-gpu`) | 445.4 | 783.2 | 313.6 | 7 |
| **T2 (final)** | **397.9** | 797.4 | 248.8 | 8 |
| **T2, zero other Chrome on box** | **400.1** | 800.3 | 250.8 | 8 |
| T2 paused 20 s | 422.0 | 823.7 | 271.6 | 8 |

**Dropped.** These made PSS no better, or worse:
- `--disable-gpu` family (591)
- background-networking family (575)
- V8 lite / optimize-for-size (581)
- low-end device mode, which matched F6a
- `--no-zygote --no-sandbox` (612, and it gives up the sandbox)
- HistoryEmbeddings off (583)
- `--in-process-gpu` (+47 in T1)

**Floor.** Three processes are essential:

| Process | PSS (MiB) |
|---|---|
| browser | 132 |
| Widevine CDM | 93 (82 USS) |
| renderer | 86 |
| **Total** | **≈310** |

That is already over budget before anything optional.

**Startup.** On a cold profile, the first usable key arrives ≈1.36 s after launch and first audio at ≈1.0 s.

## Final tuned flags (T2)
```
--headless=new --no-first-run --no-default-browser-check --autoplay-policy=no-user-gesture-required
--disable-extensions --disable-component-extensions-with-background-pages --renderer-process-limit=1
--enable-features=NetworkServiceInProcess2
--disable-features=OptimizationHints,WebUIOmniboxPopup,WebUIOmniboxAimPopup,AudioServiceOutOfProcess
```

## Sabotage proof
With the license URL changed to `…/no_auth_SABOTAGED`, both S and T2 came back **VOID**:
- `currentTime` frozen at 12.25 s
- no `usable` key
- `shaka:6007`, and 3 license requests with 0 responses

Sintel's first ~12 s is clear audio and plays without a license. "It played" alone would have been a false PASS. The key-status check is what proves DRM.

## Did-it-run evidence (T2)
- `com.widevine.alpha`, with 5 keys `usable`.
- One SourceBuffer, `audio/mp4; codecs="mp4a.40.2"`, with `videoCodec: null` and `isAudioOnly()` true.
- currentTime moved from 16.4 s to 78.2 s.
- musickit.js loaded.
- The PipeWire stream was linked only to a transient null sink, so nothing reached the real S/PDIF output. The default sink was unchanged afterwards.

## Caveats
- **PSS deflation was negligible:** 400.1 with no other Chrome running vs 397.9 with it.
- **Proxy ≠ MusicKit.** This used Shaka playing Sintel's Widevine AAC stream, with musickit.js loaded but unconfigured. Real MusicKit adds its own player and API work, so expect equal or higher numbers.
- **Inference:** the CDM's ~82 MiB USS is Widevine itself, so a native Rust CDM host would likely pay a similar cost.
- **Firefox was skipped:** it requires downloading the Widevine GMP into its own profile.

## Re-run
```
cd /home/norm/projects/Personal/apple-music-client/spikes/01-sidecar-memory
./run.sh T2-verify T2.json --warmup 15 --sample 60 --pause 20 && python3 -I report.py results/T2-verify.json
./run.sh sabotage-verify T2.json --lic https://cwip-shaka-proxy.appspot.com/no_auth_SABOTAGED --first-audio-timeout 45 && python3 -I report.py results/sabotage-verify.json
```
Raw JSON for every run is in `spikes/01-sidecar-memory/results/`.
