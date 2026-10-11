# Plan: the Bramley app shell (2026-10-10)

Nate's order holds; **no hard dependency problem**. S2 is split a/b/c to fit one session each. Soft dependencies: S2 uses the developer-token file, the library screens need S3's user token, and S3 and S4 rerun S2c's gate.

**Conventions:** clippy `-D warnings` and `cargo test --workspace` pass. Fixtures are synthetic (no Apple JSON or art committed). UI stories attach `bramley --screenshot` PNGs, with synthetic covers in public PRs (art in public screenshots is grey under section D) and real ones sent to Nate privately. An omitted field means none. Budgets (source/tests/fixtures lines) are caps; the last round ran 48% over.

## S0. Break down fastframe's +26 MiB
- **Variants** (in `spikes/03-native-cdm-and-ui/ui-baseline`): bare with the same 1,000-row list; full fastframe; `system_fallbacks(false)`; no icons; fonts only; plus `fastframe-emoji` (absent from the 85.4 build); and a positive control, bare plus 20 MiB of touched ballast.
- **Lead:** fallback faces are read into a heap `Vec` (`fastframe-fonts/src/system.rs`), and this box's `NotoSansCJK-Regular.ttc` is 19.5 MB.
- **PREREG:** two runs per variant under `sg render`, within 1 MiB of each other. The ballast must read +20±2, otherwise VOID. Toggle deltas sum to within ±4 of the total. Each bin logs its components and fallback bytes; radeonsi; frames > 0.
- **AC:** RESULTS gives the table, the font inventory, and one named recommendation with its MiB. CJK can't simply be dropped, because titles are multilingual; lazy-load it or memory-map it.
- **Risk:** the cost depends on which fonts are installed.
- **Budget** 200/0/0. **Deps:** none.

## S1a. Engine player API
- **What:** `crates/engine/src/player.rs`, `js/player.js` (moved), and `bramley-musickit` refactored onto it.
  - `Player::spawn(cfg, tokens, waker) -> (Sender<Command>, Receiver<Snapshot>)`.
  - `Command`: SetQueue{Songs|Album|Playlist, start}, Play, Pause, Seek, Next, Previous, Volume, Shutdown.
  - `Snapshot`: item id and title, position, duration, state, and `error: Option<SignInNeeded|Playback|LoadFailed>`.
  - `Engine` is `!Send`, so it is built inside the thread. The loop uses `recv_timeout(tick_wait)`.
  - Position comes from the native `currentTime`, since MusicKit's is whole seconds.
- **AC:**
  1. Arguments go through `serde_json`; the id `x");globalThis.pwned=1;("` leaves `pwned` undefined.
  2. `grep -rn media-user-token crates/*/src crates/*/js --exclude-dir=tests` returns exactly one hit (the log's "one function").
  3. `measure-musickit.sh` PASSES at ≤112, and sabotage A still says "sign-in needed".
  4. A bad song id gives a `Playback` error within 15 s.
  5. A test counts waker calls.
- **Risk:** the `innerHTML` follow-up masks `mediaPlaybackError` and may block AC4. If it does, fix it here.
- **Budget** 350/200/0. **Deps:** none. **Compliance:** rule 3.

## S1b. Shipped-feature guard
- **Goal:** prove mechanically that `bramley` ships with no test-only feature.
- **Why a naive grep fails (probed):** release binaries are stripped (`nm`: no symbols), the sabotage build has 0 `sabotage` strings, and the featureless `bramley-play` has `enableDiagnostics` twice through `platform.js`.
- **What:**
  - a `#[cfg(feature)] #[used] static` marker for each non-shipped feature: engine `sabotage` and `diagnostics` now, app `dev` (S2b) and `gate-sabotage` (S2c) as they land. Any story adding such a feature must add its marker;
  - `scripts/check-shipped-features.sh`, which checks every marker;
  - the diagnostics hook moved into `js/diagnostics.js`, loaded only under that feature.
- **AC:**
  1. The script exits 1 on `bramley-musickit --features sabotage,diagnostics`. If the marker doesn't survive LTO, make it reachable.
  2. `strings target/release/bramley-play | grep -c enableDiagnostics` prints 0.
  3. After S2a: the script exits 0 on `bramley`, and exits 1 after `cargo build --release --workspace --all-features` (feature unification).
- **Budget** 60/40/0. **Deps:** S1a.

## S2a. App skeleton
- **What:**
  - `crates/app`, bin `bramley`: eframe glow, with fastframe set up per S0;
  - the player thread, a hard-coded Play button, and a now-playing bar (play, pause, seek, skip);
  - the crmne `[patch]` from ui-baseline;
  - `--screenshot <png> --after <s>` (Spotifast's `drive_shot`), `--status <json>`, and a shipped `--play <song-id>` for unattended runs (see the compliance flags).
- **AC:**
  1. Reducer tests map Action to Command.
  2. S1b AC3 holds.
  3. On weston, the screenshot is a 1100×720 PNG that isn't a single colour.
  4. `bramley --play 6781024437` shows decrypted seconds > 15 in the status.
- **Risk:** weston presents only one frame without damage, so repaint every second.
- **Nate sees:** the bar mid-song.
- **Budget** 400/120/0. **Deps:** S0, S1b.
- **Deviation (2026-10-10, coordinator's call):** no hard-coded Play button and `--play` is dev-only; the release build's first user-initiated play path is S11's CLI.

## S2b. Cover grid and cache policy
- **What:** 50 covers from `/v1/catalog/us/charts?types=albums&limit=50`.
- **Policy:**
  - fill the `{w}x{h}` template at the cell's pixel size;
  - decode on 2 workers;
  - `ctx.load_texture`, then drop the RGBA;
  - no egui_extras loaders (Spotifast's `images.rs` shows they keep CPU and GPU copies);
  - no disk cache until Nate rules.
- **AC:**
  1. 160 pt at 2× gives `320x320bb.jpg`.
  2. Against the fixture server, the status shows `texturesUploaded: 50, decodedBytesHeld: 0`.
  3. The API-base override exists only under the `dev` feature.
- **Risk:** the egui-fork patch versions must match.
- **Budget** 300/150/60. **Deps:** S2a. **Compliance:** art appears only inside the player (section D).

## S2c. Whole-app memory gate
- **Goal:** pre-register and run the ≤200 MiB whole-process gate.
- **Files:**
  - `docs/app-shell-memory-PREREG.md`;
  - `scripts/measure-app.sh`, in the style of `measure-musickit.sh`;
  - `scripts/app/verdict.py`.
- **Headroom:** about 14 MiB (200 − 85.4 − 100.4).
- **PASS:** PSS median ≤200 over 60 s of steady playback, with the shipped features, covers visible, on weston/radeonsi. No marginal band.
- **Did it run:** radeonsi, 50 textures, ≥50 decrypted seconds and ≥30 frames during the sample, 1 process, null sink.
- **Sabotage:**
  - **A:** the `gate-sabotage` feature retains RGBA, and PSS must rise by at least 80% of 50·w·h·4.
  - **B:** an empty token dir must FAIL. After S3b, use an empty keyring service (`BRAMLEY_KEYRING_SERVICE`).
  - **C:** an unreachable art host must be VOID.
- **Named false positive:** textures in VRAM don't show in PSS. Report `mem_info_vram_used`.
- **AC:** the verdicts are committed, and budget.md gets a row.
- **Budget** 220/0/0. **Deps:** S2b.

## S3a. Loopback sign-in
- **What:** serve the spike-2 page on `127.0.0.1:0`, open the browser, and run Apple's `authorize()` there.
- **Server rules:** a 128-bit nonce path, Host must be `127.0.0.1:<port>`, a single-use POST, a 10-minute lifetime. The URL is always printed: the dev box has no browser, and `ssh -L` needs the port.
- **AC:** bad nonce 404; bad Host 403; repeat POST 410; the literal token appears 0 times in a captured log (`eyJ` misses it).
- **Nate does:** `ssh -L 127.0.0.1:P:127.0.0.1:P devbox`, opens the printed `http://127.0.0.1:P/…` URL and signs in. Use 127.0.0.1, not `localhost`: spike 2 proved only 127.0.0.1.
- **Budget** 300/200/0. **Deps:** S2a. **Compliance:** honest origin.

## S3b. Keyring and sign-in-needed
- **What:**
  - the OS keyring, behind `cfg` modules, following Spotifast's `credentials.rs`;
  - import the old token file into the keyring, read it back, then delete the file (one copy; see decisions.md, 2026-10-10 user-token storage);
  - `SignInNeeded` shows a loud banner, then runs S3a, then respawns the player;
  - with no Secret Service on the bus, keep the token in an atomic 0600 file and show "file" as the storage mode in status and the UI. Any other keyring error, timeout or cancelled unlock is a loud error and never falls back to plaintext.
- **AC:**
  1. A corrupted token shows the banner within 20 s, with 0 decrypted seconds.
  2. `grep -F -f <(secret-tool lookup …)` over logs, status and `target/` finds nothing.
  3. The S2c rerun PASSES, and the keyring's delta is recorded.
- **Risk:** Spotifast's store pulls in tokio and zbus.
- **Nate sees:** the banner.
- **Budget** 200/120/0. **Deps:** S3a.

## S4a. CI token publishing
- **Where:** GitHub Pages, `natevick.github.io/bramley/developer-token.json` → `{token, iat, exp}`. Pages gives a stable URL behind a CDN. A token release asset would become "latest" and collide with fastframe-update's self-update.
- **What:**
  - `.github/workflows/developer-token.yml`, weekly plus dispatch;
  - `scripts/mint-developer-token.py`: ES256, `kid`/`iss`, 180 days, no origin;
  - verify against the public key, and require a live catalog 200.
- **AC:**
  1. A throwaway-key mint verifies.
  2. `actionlint` is clean.
  3. The dispatch run is green, with `exp` ≥150 days out.
  4. Dispatch with `sabotage: true` fails.
- **Risks:** GitHub's docs confirm public-repo schedules stop after 60 inactive days; 180-day tokens leave ≥150 days, the app warns at 14, and a runbook covers `gh workflow enable`. This deploy owns the whole Pages site.
- **Nate only:** add the three secrets; enable Pages (Actions source); **approve a compliance log entry, because rule 7 keeps the `.p8` in 1Password. This blocks merge.**
- **Budget** 120/60/0. **Deps:** none (can run in parallel).

## S4b. App token fetch
- **What:** a cache at `$XDG_CACHE_HOME/bramley/developer-token.json`, fetched on a worker when it is missing or has under 30 days left.
- **AC (fixture tests):** 100 days left → 0 requests; 20 → fetch; 20 plus a 500 → cache and warn; none plus a 500 → loud error; malformed → cache untouched; 10 days plus a failed fetch → a visible warning and `devTokenDaysLeft` in status; a developer-token 401 never reads as "sign in". The S2c rerun PASSES.
- **Budget** 200/150/20. **Deps:** S4a, S2a.

## S5. Apple Music API client
- **What:** `apple/{client,models}.rs` on `ureq` (the engine's TLS stack): storefront, search, library playlists/albums/songs (following `next`), catalog album and playlist, playlist tracks.
- **AC:**
  1. A host and path allowlist (rule 4).
  2. `Music-User-Token` is sent only on `/v1/me`, with Bramley's UA and no `Origin` (rule 5).
  3. Typed 401 and 429 errors.
  4. `cargo tree -p bramley | grep -c reqwest` prints 0.
  5. A live smoke test under `cfg(bramley_tokens)`.
- **Budget** 400/250/150. **Deps:** S3b, S4b.

## S6. Layout shell
- **Goal:** Spotifast's layout with navigation.
- **Files:** `ui/{sidebar,content,player_bar,queue_panel}.rs`, `app.rs` (`Route`, back stack). The S2 grid becomes home.
- **AC:** navigation reducer tests, plus screenshots at 1100×720 and 800×600.
- **Budget** 400/150/0. **Deps:** S2c.

## S7. Search
- **Goal:** catalog search with song, album and playlist sections.
- **Files:** `ui/search.rs`, `app.rs` (`Route::Search`, `Action::Search`, debounce).
- **AC:** 3 keystrokes within 300 ms send 1 request; a stale response is dropped; a song click plays it; a screenshot.
- **Budget** 300/150/40. **Deps:** S5, S6.

## S8. Album and playlist detail
- **Goal:** header art and a track list, with play-all and play-from-track.
- **Files:** `ui/detail.rs`, `app.rs` (`Route::Album`, `Route::Playlist`).
- **AC:** starting at track 3 makes the snapshot's item track 3 within 10 s (live).
- **Risk:** `l.`/`p.` library ids in `setQueue` are unproven.
- **Budget** 300/150/40. **Deps:** S7.

Addition 2026-10-11 (Norm, from the S6 review): S8 makes Home's cover grid playable. Clicking a cover opens its album detail, with play. That keeps album art inside a playback context (compliance §D).

## S9. Library lists
- **Goal:** sidebar entries for library playlists, albums and songs, paged lazily.
- **Files:** `ui/library.rs`, `ui/sidebar.rs`.
- **AC:** 250 fixture items over 3 pages give 250 rows via `show_rows`, and the live count equals `meta.total`.
- **Budget** 300/150/40. **Deps:** S5, S6.

## S10. Queue panel
- **Goal:** show the queue and jump within it.
- **Files:** `engine/src/player.rs` and `js/player.js` (the snapshot gains `queue.items`), `ui/queue_panel.rs`. A click calls `changeToMediaAtIndex`.
- **AC:** a stub engine test; live, clicking item 3 shows that item's id; a screenshot.
- **Budget** 250/120/0. **Deps:** S8.

## S11. CLI control (added 2026-10-10, Nate)
- **Goal:** `bramley <command>` drives the running app through fastframe-instance's single-instance channel.
- **Commands:** `play [id]`, `pause`, `toggle`, `next`, `prev`, `seek <s>`, `volume <0-100>`, `now` (JSON), `queue add <id>`, `search <term>` (JSON; needs S5).
- **Behaviour:** it exits non-zero with a clear message when the app isn't running. A second launch with no arguments shows the window.
- **Depends on:** S2a for the playback commands, S5 for search.
- **Tests:** a channel round-trip with a fake app.
- **Budget:** ~250 source / ~200 tests.

## S12. MCP server (added 2026-10-10, Nate)
- **Goal:** `bramley mcp` is a stdio MCP server whose tools mirror S11 (search, play, pause, next, now_playing, queue_add, library_list). It talks to the running app over the same channel.
- **Implementation:** use the official Rust MCP SDK (`rmcp`) if it fits the dependency rules. Otherwise use a minimal JSON-RPC over stdio.
- **Constraints:** stdio only, no network listener. The first run prints the metadata disclosure from the compliance log. Never autoplay; every play tool call is an explicit request.
- **Acceptance:** an MCP client (e.g. `claude mcp add bramley -- bramley mcp`) lists the tools and runs `search` and `play` against the running app.
- **Depends on:** S11 and S5.
- **Budget:** ~300 source / ~200 tests.
- **F9 harness wiring (2026-10-11, coordinator's call):** wiring F9 into `scripts/acceptance/gate.py` belongs to S13, not S12. S12's checks live in `scripts/app/mcp-session.py`, which counts against S13's harness cap.

## S13. Acceptance harness (added 2026-10-10)
- **Goal:** `scripts/acceptance.sh` implements docs/acceptance.md exactly: every F, R and control check, evidence files, and private screenshots. It drives the shipped binary only through the S11 CLI and S12 MCP.
- **Order:** S11 (CLI) moves up to right after S2a, so every later story can be checked by the harness. Each story from S2c on adds its checks to the harness as it lands, and done means the relevant acceptance checks pass.
- **Budget:** harness ≤ 600 lines (bash plus python), no app code.

## Compliance flags
- **S4a (blocking):** rule 7 versus keeping the key in GitHub secrets.
- **S1a:** rule 3, and the seed in one location.
- **S2b/S5:** section D's limits on using art and metadata; the rule 4 allowlist; an art disk cache waits on Nate.
- **S3a:** honest origin, and no tokens in logs.
- **S2a, for Nate to decide (rule 8):** `--play <id>` starts playback without a click. A user typing the flag is arguably user-initiated; autoplay on launch is not, so it is never the default.

**Deferred:** MPRIS, the tray, emoji (unless S0 clears it), and the Windows/macOS builds.
