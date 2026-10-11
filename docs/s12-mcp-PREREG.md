# S12 MCP server: pre-registration (2026-10-11)

Written before any live run. The driver is `scripts/app/mcp-session.py`, run under `sg render`. It drives the release `bramley` only through `bramley mcp`, a stdio JSON-RPC client in the driver, and reuses `covers-session.py`'s `session()` (weston GL plus a temporary null sink), `isolated()` (config dir and keyring service) and `Guard` (kills the app if its stream links anywhere but the null sink).

Binary: `cargo build --release -p bramley` → `target/release/bramley`. It must be fresh against HEAD, and `scripts/check-shipped-features.sh` must exit 0 on it.

The app and `bramley mcp` share a private `XDG_RUNTIME_DIR` under `target/`, so the run can't reach or be reached by any other Bramley on the box. Wayland and PulseAudio are passed by absolute path (`WAYLAND_DISPLAY`, `PULSE_SERVER`).

## Did it run (otherwise VOID)
- The app paints frames on `AMD`, not `llvmpipe`.
- The guard has at least one clean poll, and the null sink exists, before `play` is sent.
- The default sink is the same before and after, and the null sink is gone afterwards.

## Checks (signed in)
1. **No app:** before the app starts, `now_playing` is a tool error containing "not running".
2. **Handshake:** `initialize` echoes the client's `protocolVersion`, and `tools/list` names exactly `search play pause resume next previous seek queue_add now_playing library_playlists`.
3. **Search:** `search {"term": "bohemian rhapsody"}` is not an error and returns at least one song. Whether 6781024437 is among them is recorded.
4. **Play:** `play {"id": "6781024437"}` is not an error. Within 90 s, `now_playing` shows `playing` true, `itemId` 6781024437, `problem` null and position > 35. The app's stream links only to the null sink throughout (the guard never fires, and every target it saw is the null sink).
5. **Queue:** `queue_add {"id": "6781079518"}` is not an error.
6. **Pause:** within 5 s, `now_playing` shows `state` Paused, `pending` false and `playing` false.
7. **Next:** not an error. Report only: `state`, `itemId` and `playing` 3 s later. S11 found that MusicKit starts playback from Paused.
8. **Library:** `library_playlists` is not an error and `total` is an integer. Only `total`, `returned` and `more` are recorded, never names or ids.
9. **mcp process:** while it serves, `/proc/<pid>/fd` has no socket listed in `/proc/<pid>/net/{tcp,tcp6,udp,udp6,raw,raw6}`. Its first-run stderr carries the disclosure and `mcp-disclosed` exists. A second `bramley mcp` against the same config prints nothing on stderr through `initialize`.
10. Report only: the median PSS of 10 one-second samples while playing, compared against S11's 183.2 MiB. The `bramley mcp` process's own PSS.

The run finishes with `pause`, then the driver stops `bramley mcp`, the app, weston and the null sink, which are the only processes it started.

## Sabotage (the harness must be able to say FAIL)
- **Signed out:** the same driver with `isolated(..., signed_in=False)`. `play` must come back as a tool error ("sign in"), check 4 must FAIL, and the verdict must be FAIL.

## Amendment (2026-10-11, after r1–r3 and before r4/r5)
- **Order:** `queue_add` moves to after `pause`. Check 5 becomes: `queue_add` is not an error, and `now_playing` 2 s later still shows Paused with `playing` false. This tests the tool description "Does not start playback". `next` follows as before.
- **Check 4, F4's latency bar** (F9 is "same bar as F4"): the time from sending `play` to the start of audio, taken as the reply time of the first `now_playing` with `playing` true and position > 0 minus that position, is at most 3 s.
- **Bad sink (r5):** a third mode with `PULSE_SINK` naming a sink that does not exist and the guard armed. After `play`, within 25 s, `now_playing` shows `state` Error, `playing` false and a non-null `problem`, and the guard saw no target at all. That makes the mode PASS; anything else is FAIL. Checks 3 to 9 aren't scored in this mode.
