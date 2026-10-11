# S5 Apple Music API client: pre-registration (2026-10-10)

Fixed before any live run. Driver: `scratch/s5/live.sh` (not committed). The app is the fresh release build of the branch tip (`cargo build --release -p bramley`). It runs on weston headless `--renderer=gl` under `sg render`, is never told to play, and is driven only through `bramley <command>`. Tokens are read from `~/.config/bramley` at runtime.

## Did it run (otherwise VOID)
- `bramley now` answers within 15 s with `framesPainted` > 0.
- The sabotage run below fails. If it passes, the harness is not measuring the app, and every pass is VOID.

## Checks
1. **F3 search:** `bramley search "bohemian rhapsody"` is run 5 times, timed from exec to exit. Each must exit 0 and its JSON must hold a song with id `6781024437`. Pass: the median is under 2 s. All 5 times are reported.
2. **Library (F7 groundwork):** the live smoke test `live_search_and_the_largest_library_playlist` is built with `--cfg bramley_tokens`. It lists every library playlist (following `next`) and reads each one's track total. It then pages the largest playlist's tracks, and the count listed must equal `meta.total`. Pass: the test passes. Only counts are reported; names never are.
3. **Guard:** `scripts/check-shipped-features.sh target/release/bramley` exits 0.
4. **No token in the log:** a search run under `RUST_LOG=trace` puts ureq or rustls lines in app.log (otherwise VOID). A grep for either token, `eyJ`, `Bearer ` or `Music-User-Token:` with a value finds nothing in app.log or the CLI output.

## Sabotage control
With `BRAMLEY_CONFIG_DIR` set to a copy whose developer token is corrupted, `bramley search "bohemian rhapsody"` must exit non-zero. Its output must contain neither real token.
