# S12 MCP server: results (2026-10-11)

Pre-registration: `docs/s12-mcp-PREREG.md`. Binary: `target/release/bramley` built at c9b6046 (the code tip; the harness and docs commit after it doesn't change the binary). `check-shipped-features.sh` exits 0 on it. Every run used weston headless `--renderer=gl` under `sg render`, with renderer `AMD Radeon RX 5700 XT (radeonsi, navi10, ACO, DRM 3.64, 7.0.0-31-generic)`. Driver: `scripts/app/mcp-session.py`, a stdio JSON-RPC client. `claude mcp` was not used, because it has no non-interactive way to call a tool.

Evidence stays on the dev box, under `target/s12/live/` in the S12 worktree: `r1-signed-in/`, `r2-signed-in/` and `r3-signed-out/`. Each holds `verdict.json`, the `mcp*.jsonl` transcripts and stderr, `app.log`, `status.json`, `weston.log` and `session.verdict.json`. Library playlist names are redacted in the transcript before it is written; only counts are kept.

## Live proof (signed in, r2): PASS, 9/9

| Check | Result |
|---|---|
| 1 No app | `now_playing` is a tool error: "Bramley is not running (nothing at …/xdg/bramley); start it first" |
| 2 Handshake | `protocolVersion` 2025-06-18 echoed; `tools/list` returns the 10 tools in order |
| 3 Search "bohemian rhapsody" | not an error: 10 songs, 7 albums, 0 artists, 0 playlists; 6781024437 is among the songs |
| Routing before play | 3 clean guard polls; the null sink exists |
| 4 play 6781024437 | the ack is not an error (`pending` true, `playing` false). `now_playing` reached `playing` true, Bohemian Rhapsody / Queen, position 35.66 s, problem null. The guard saw only `bramley-s12-null-157129` and never fired |
| 5 queue_add 6781079518 | not an error |
| 6 pause | Paused, `pending` false, within about 1 s |
| 7 next (report: from Paused) | **playback started**: 3 s later, Playing at 2.59 s on 6781079518 "Don't Stop Me Now", `playing` true. This matches S11 and the tool description |
| 8 library_playlists | total 34, returned 34, more false |
| 9 mcp process | 0 sockets of any kind in `/proc/<pid>/fd`, mid-run and at the end. The disclosure was on the first run's stderr and `mcp-disclosed` was written. The two later runs printed nothing. Exit 0 on stdin close |

The run ended with `pause`. The default sink was unchanged and the null sink was gone afterwards.

**PSS while playing (report only):** the app's median of 10 one-second samples was 170.8 MiB (r1: 169.7), against S11's 183.2 on an older tip. One session with no reference binary, so this is not an R1 reading. The `bramley mcp` process itself was 3.6 MiB.

## Sabotage (signed out, r3): FAIL, as required

`play` came back as a tool error, "sign in needed: nothing was played". `now_playing` reported `state` SignInNeeded, `playing` false, problem "sign in needed: run `bramley sign-in`". The guard saw no stream at all. Checks 3 to 8 failed, so the verdict was FAIL. Checks 1, 2 and 9 still passed, since they don't need a user token.

## Harness defect (r1)

r1 played and passed checks 1 to 8, but check 9 failed. The driver looked for the first-run disclosure on the second `bramley mcp` process. The no-app probe is the first run against a fresh config, so it had already shown and recorded the disclosure. The driver now reads the probe's stderr as the first run and requires the later runs to be silent. The product behaved as pre-registered in both runs.

## Unit and mutation proof

- `cargo test --workspace` passes: the app has 73 unit tests, plus 1 integration test (`crates/app/tests/mcp_stdio.rs`). Clippy `-D warnings` and `cargo fmt --check` are clean.
- 28 mutations, each run against the test that owns its behaviour after a green baseline for every filter: **28 killed, 0 survived**. They cover:
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
  - CLI: `library playlists` not parsed, not routed, its reply dropped;
  - a TCP listener and a UDP socket opened in `run`.

## Dependency

rmcp 3.5.1 (the official SDK) with default features off, `server` and `transport-io` only, plus tokio `rt`. This adds 14 crates to `Cargo.lock`: rmcp, schemars and its derive, chrono, tokio-util, tokio-macros, pastey and others. Neither hyper nor reqwest is in the tree. The release binary grew from 20.90 MB to 22.54 MB.

## Not done / for Nate

- The disclosure goes to stderr, which most MCP clients send to a log the user may never open. It is also the server's `instructions`, so the agent sees it, but whether a human ever reads it depends on the client.
- `library_playlists` returns only the first page (100). `more` says when there are more.
- Windows and macOS were not cross-checked: only the Linux target is installed on this box.
- F9 is not wired into `scripts/acceptance/gate.py`. The driver is shaped to be called from there, but that is S13's harness work.
