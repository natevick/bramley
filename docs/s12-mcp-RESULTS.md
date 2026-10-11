# S12 MCP server: results (2026-10-11)

Pre-registration: `docs/s12-mcp-PREREG.md`. Binaries: r1–r3 ran on `target/release/bramley` built at c9b6046, and r4–r5 on the build at 4931c57 (the code tip, with the CLI refusal of `library playlists`). `check-shipped-features.sh` exits 0 on it. Every run used weston headless `--renderer=gl` under `sg render`, with renderer `AMD Radeon RX 5700 XT (radeonsi, navi10, ACO, DRM 3.64, 7.0.0-31-generic)`. Driver: `scripts/app/mcp-session.py`, a stdio JSON-RPC client. `claude mcp` was not used; the brief's fallback, a small stdio driver, was used instead.

Evidence stays on the dev box, under `target/s12/live/` in the S12 worktree: `r1-signed-in/`, `r2-signed-in/`, `r3-signed-out/`, `r4-signed-in/` and `r5-bad-sink/`. Each holds `verdict.json`, the `mcp*.jsonl` transcripts and stderr, `app.log`, `status.json`, `weston.log` and `session.verdict.json`. Library playlist names are redacted in the transcript before it is written; only counts are kept.

## Live proof (signed in, r4, amended order): PASS, 9/9

| Check | Result |
|---|---|
| 1 No app | `now_playing` is a tool error: "Bramley is not running (nothing at …/xdg/bramley); start it first" |
| 2 Handshake | `protocolVersion` 2025-06-18 echoed; `tools/list` returns the 10 tools in order |
| 3 Search "bohemian rhapsody" | not an error: 10 songs, 7 albums, 0 artists, 0 playlists; 6781024437 is among the songs |
| Routing before play | 2 clean guard polls; the null sink exists |
| 4 play 6781024437 | the ack is not an error (`pending` true, `playing` false). **Audio started 1.59 s after `play` was sent** (F4's bar is ≤ 3 s). `now_playing` reached `playing` true, Bohemian Rhapsody / Queen, position 35.91 s, problem null. The guard saw only `bramley-s12-null-210677` and never fired |
| 6 pause | Paused, `pending` false, within about 1 s |
| 5 queue_add 6781079518 (while Paused) | not an error; 2 s later still Paused at 46.02 s, `playing` false. "Does not start playback" holds |
| 7 next (report: from Paused) | **playback started**: 3 s later, Playing at 2.63 s on 6781079518 "Don't Stop Me Now", `playing` true. This matches S11 and the tool description |
| 8 library_playlists | total 34, returned 34, more false |
| 9 mcp process | 0 sockets of any kind in `/proc/<pid>/fd`, mid-run and at the end. The disclosure was on the first run's stderr and `mcp-disclosed` was written. The two later runs printed nothing. Exit 0 on stdin close |

The run ended with `pause`. The default sink was unchanged and the null sink was gone afterwards. r2 ran the pre-amendment order (queue_add while Playing) and also passed 9/9. Audio started there about 1.8 s after `play` (position 1.22 at 3.02 s).

**PSS while playing (report only):** the app's median of 10 one-second samples was 156.3 MiB in r4, 170.8 in r2 and 169.7 in r1, against S11's 183.2 on an older tip. These are single sessions with no reference binary, so they are not R1 readings, and the 14 MiB spread between runs is noise this method can't resolve. The `bramley mcp` process itself was 3.5 MiB.

## Bad sink (r5): PASS

`PULSE_SINK` named a sink that does not exist, and the guard was armed. `play` was acked. `now_playing` then showed `state` Error, `playing` false, problem "the audio output could not be opened", while `playerState` was still Playing at 0.0 s. That is S11's trap, reported honestly. The guard saw no stream at all.

## Sabotage (signed out, r3): FAIL, as required

`play` came back as a tool error, "sign in needed: nothing was played". `now_playing` reported `state` SignInNeeded, `playing` false, problem "sign in needed: run `bramley sign-in`". The guard saw no stream at all. Checks 3 to 8 failed, so the verdict was FAIL. Checks 1, 2 and 9 still passed, since they don't need a user token.

## After the review (r6, r7): PASS

Binary: release at c0e0160; the guard exits 0. Pre-registered as amendment 2.

**r6** (signed in, r4 sequence): PASS, 10/10.
- Audio started 1.79 s after `play` and reached 35.66 s. Only the null sink was linked.
- queue_add while Paused stayed Paused. `next` from Paused started "Don't Stop Me Now". library_playlists returned total 34.
- **Check 10:** `agentNotice` was true 2 s after the first MCP search and still true at the end. `mcp-disclosed` was never written, because nobody dismissed the notice.
- **Check 9:** 0 sockets on the mcp process. All three `bramley mcp` runs printed the disclosure, as they should while the notice is undismissed.
- PSS while playing: 145.4 MiB (report only).

**r7** (dev build, `--screenshot`, private): one MCP `now_playing` after the first frame. The PNG shows the notice, reading "An agent is connected to Bramley", the three lines and a "Got it" button, above the library grid. The final status had `agentNotice` true. The PNG holds real album art, so it stays at `target/s12/live/r7-shot/notice.png` and is not committed.

**Dismissal was not driven live.** Headless weston gives the driver no input device. A unit test (`an_agent_request_raises_the_app_notice_and_only_dismissal_records_it`) covers it instead:
- it finds "Got it" by its accesskit label in the real `App::draw`;
- clicks it with egui pointer events;
- checks the marker appears only after the click.

The mutation that unwires the click is killed by it.

## Harness defect (r1)

r1 played and passed checks 1 to 8, but check 9 failed. The driver looked for the first-run disclosure on the second `bramley mcp` process. The no-app probe is the first run against a fresh config, so it had already shown and recorded the disclosure. The driver now reads the probe's stderr as the first run and requires the later runs to be silent. The product behaved as pre-registered in both runs.

## Unit and mutation proof

- `cargo test --workspace` passes: the app has 73 unit tests, plus 1 integration test (`crates/app/tests/mcp_stdio.rs`). Clippy `-D warnings` and `cargo fmt --check` are clean.
- After the review: **40 mutations, 40 killed**, each after a green baseline. A filter matching zero tests now counts as red; that exposed two earlier "survivors" that had run against a renamed or wrong test. The new mutations are:
  - B1: no raise on an agent request, no raise on a library request, the origin dropped in the handler, the origin dropped in mcp, the click unwired, the marker written on stderr print, the marker ignored by the app, the untrusted sentence dropped;
  - L1: no mutex; L3: no term cap; L4: a logger on the mcp path with rmcp unfiltered;
  - T1: `next` ignored by `Page`; T2: `artists` renamed.
- Before the review: 29 mutations, 29 killed. They cover:
  - handshake: no tools capability, no instructions;
  - tools/list: empty, no `required`;
  - validation: `cli::parse` skipped, extra arguments allowed, `types` unchecked, a non-string id;
  - mapping: `resume` sent as `toggle`;
  - error mapping: a swallowed channel error, an ignored refusal, an unknown tool turned into a tool error;
  - now_playing: banner ignored, pending ignored, state not overridden, the whole status dumped;
  - search: artwork kept, `types` ignored;
  - library: `more` ignored;
  - disclosure: never recorded, failure silent, never printed;
  - the `next` caveat dropped from the description;
  - CLI: `library playlists` not parsed, not routed, its reply dropped, or allowed on the plain CLI;
  - a TCP listener and a UDP socket opened in `run`.

## Dependency

rmcp 3.5.1 (the official SDK) with default features off, `server` and `transport-io` only, plus tokio `rt`. This adds 14 crates to `Cargo.lock`: rmcp, schemars and its derive, chrono, tokio-util, tokio-macros, pastey and others. Neither hyper nor reqwest is in the tree. The release binary grew from 20.90 MB to 22.54 MB.

## Not done / for Nate

- The disclosure goes to stderr, which most MCP clients send to a log the user may never open. It is also the server's `instructions`, so the agent sees it, but whether a human ever reads it depends on the client.
- `library_playlists` returns only the first page (100). `more` says when there are more.
- Windows and macOS were not cross-checked: only the Linux target is installed on this box.
- **By the plan's definition, S12 is not done until F9 is in the acceptance harness.** S13 says every story from S2c on adds its checks to `scripts/acceptance.sh` and is done when they pass. F9 is not yet wired into `scripts/acceptance/gate.py`. `scripts/app/mcp-session.py` (254 lines of Python) holds the checks and counts against S13's 600-line harness cap. The coordinator decided on 2026-10-11 that the wiring belongs to S13; the plan records it.
- The plain CLI refuses `bramley library playlists`. This is not a protection. Any same-user process can ask the instance socket for it. See the 2026-10-11 trust-boundary entry in compliance.md, which also covers the macOS and Windows re-checks.
- **L5 (open):** while the startup keyring check runs (up to 60 s with a locked keyring), `now_playing` shows no problem, but `play` is refused with "sign in needed".
- **L7 (open):** seeking past the end while Paused hasn't been probed live.
- **Minimized window (open; S13 measures it):** the inbox drains only inside `App::logic`. If a minimized or occluded window gets no frames, every CLI and MCP call would come back "not applied". Every run so far had a visible headless surface.
- **F9 by proxy:** check 4 uses MusicKit's `position` > 35 plus the routing guard, not a native decrypt counter. No status field reports one.
