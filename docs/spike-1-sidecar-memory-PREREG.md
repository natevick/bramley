# Spike 1: can a Chrome playback sidecar stay small? (pre-registered 2026-10-09)

Written before any measurement. The bar below cannot move after data arrives.

## Question
How much memory does a headless Chrome take while it plays Widevine-protected audio-only content from a minimal page, both stock and with tuned flags?

## Proxy
There are no Apple credentials yet. Shaka Player plays Google's public Widevine demo asset (`angel-one-widevine`) with `manifest.disableVideo: true`, licensed through `cwip-shaka-proxy.appspot.com/no_auth`. Shaka is roughly the same JS weight as musickit.js, which bundles its own HLS/EME player. `musickit.js` v3 is also loaded from Apple's CDN on the same page so its parse cost and heap count, but it is not configured.

## Metric
- Walk the whole process tree from the launched PID; every descendant is counted.
- Per process, read `/proc/<pid>/smaps_rollup`: Pss, Rss, and USS (Private_Clean + Private_Dirty).
- Sum each across the tree.
- Sample every 2 s for 60 s of steady playback, after a 15 s warm-up.
- Report median and max of each metric.
- The headline number is **PSS**. RSS is the conservative upper bound. USS is the floor.

## Pass condition
- **Pass:** tuned-config median PSS ≤ 150 MB across the whole tree. That leaves about 50 MB for the egui UI inside a 200 MB total.
- **Marginal:** 150–200 MB.
- **Fail:** above 200 MB. Nate's 200 MB budget cannot be met with Chrome.

## Did-it-run checks
All of these are required, or the run is void.
1. The video element's `currentTime` advances past 30 s during the window. This is polled over CDP or exposed by the page.
2. A MediaKeySession reaches key status `usable`. This proves Widevine actually decrypted the content, rather than the run playing clear audio.
3. Only the audio track is buffered (`videoTracks` empty, or Shaka reports no video variant). This proves we measured audio-only.
4. Chrome's audio output is active: a Chrome node appears in `pw-dump` while playing, or the run notes that a fake output was used.
5. The process count in the tree is logged per sample. If it is 1, the tree walk failed.

## Named false positives
- **PSS deflated by other Chrome instances.** About 12 other Chrome processes run on this box. Shared `chrome` binary pages split PSS across all of them, so our PSS looks smaller than on a user's machine. Mitigation: report RSS and USS beside PSS, and treat RSS as the "user sees this" upper bound. Re-run PSS when no other Chrome is running if possible.
- **Clear playback mistaken for DRM.** Check 2 guards this.
- **Wrapper PID.** `/usr/bin/google-chrome` is a shell wrapper. Launch `/opt/google/chrome/chrome` directly, or walk from the wrapper's descendants.
- **Idle mistaken for playing.** Check 1 guards this.
- **Flag silently ignored.** Log `chrome://version` command line / `ps` args, and log the process types present (`--type=` argument) for each config.

## Configs
- **S (stock):** `--headless=new`, fresh `--user-data-dir` under /home (/tmp is out of inodes), `--autoplay-policy=no-user-gesture-required`.
- **T (tuned):** S plus memory-reduction flags (GPU off, extensions off, background networking off, network/audio/storage in-process where still supported, renderer limit 1, V8 heap cap, etc.). Record which flags actually changed the process list.
- **Optional F:** Firefox 157 headless, only if Widevine GMP is obtainable without a download dance. Otherwise skip and say so.
