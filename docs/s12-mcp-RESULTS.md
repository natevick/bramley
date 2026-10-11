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

## Harness defect (r1)

r1 played and passed checks 1 to 8, but check 9 failed. The driver looked for the first-run disclosure on the second `bramley mcp` process. The no-app probe is the first run against a fresh config, so it had already shown and recorded the disclosure. The driver now reads the probe's stderr as the first run and requires the later runs to be silent. The product behaved as pre-registered in both runs.

## Unit and mutation proof

- `cargo test --workspace` passes: the app has 73 unit tests, plus 1 integration test (`crates/app/tests/mcp_stdio.rs`). Clippy `-D warnings` and `cargo fmt --check` are clean.
- 29 mutations, each run against the test that owns its behaviour after a green baseline for every filter: **29 killed, 0 survived**. They cover:
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
- **By the plan's definition, S12 is not done until F9 is in the acceptance harness.** S13 says every story from S2c on adds its checks to `scripts/acceptance.sh` and is done when they pass. F9 is not yet wired into `scripts/acceptance/gate.py`. `scripts/app/mcp-session.py` (227 lines of Python) holds the checks and counts against S13's 600-line harness cap. `gate.py` was outside this story's file scope, so wiring it is the caller's call.
- `bramley library playlists` is refused by the plain CLI (exit 2, "shared only with an agent you connect through `bramley mcp`"), so library data leaves only through the disclosed MCP path. The instance socket itself still accepts the request from any local client that speaks the wire format. That socket lives in the user's own 0700 runtime dir, so this shrinks the advertised surface rather than adding access control.
