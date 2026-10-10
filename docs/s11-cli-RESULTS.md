# S11 CLI control: results (2026-10-10)

Pre-registration: `docs/s11-cli-PREREG.md`. Binary: `target/release/bramley` built at b4b036f (the code tip; this file is docs only). Every run used weston headless `--renderer=gl` under `sg render`, with renderer `AMD Radeon RX 5700 XT (radeonsi, navi10, ACO, DRM 3.64, 7.0.0-31-generic)`. Driven only through `bramley <command>`.

Evidence stays on the dev box (`scratch/` is gitignored): `scratch/live/final-play/`, `final-signed-out/` and `final-bad-sink/`. Each directory holds `cli.jsonl` (every command and reply), `verdict.json`, `status.json`, `app.log`, `weston.log`, and the default sink before and after.

## Live proof (signed in): PASS, 11/11, plus one report-only item

| Check | Result |
|---|---|
| play 6781024437 ack | exit 0, seq 2, engine 1, instanceGuarded true |
| Routing | the app's only stream links only to `bramley-s11-null-<pid>`; the default sink is unchanged; the null sink is gone afterwards |
| > 35 s, no banner | 35.36 s after 33.4 s, Bohemian Rhapsody / Queen, applied 2 |
| pause | Paused, applied 3 ≥ seq 3, in 0.025 s |
| seek 120 | position 120.0, applied 4, in 0.55 s |
| toggle | Playing, applied 5, in 0.025 s |
| queue add 6781079518 → next | itemId 6781079518 "Don't Stop Me Now", applied 7, in 0.55 s |
| volume 40 | applied 8, in 0.025 s |

**PSS while playing:** median of 10 samples at 1 s intervals: **PSS 183.3 MiB**, USS 160.9, RSS 211.6. Earlier runs gave 183.4 (7edf2b1) and 182.5 (92910c2). This is one session with no reference binary, so it is not an R1 reading.

**`next` while Paused (report only):** MusicKit **starts playback**. 3 s after `next` was applied from Paused, the state was Playing at 3.06 s on the next queued item (6781024437). The ⏭ button behaves the same way. S12's `next`/`prev` tool descriptions must say so.

## Controls: PASS

| Control | Result |
|---|---|
| `now` with no app running | exit 3, "Bramley is not running (nothing at /run/user/1000/bramley)" |
| `play` with an empty config dir | exit 5, "sign in needed". Five seconds later: banner SignInNeeded, engineStarted false, Stopped, position null, 0 player threads |
| Second launch with no arguments | exit 0 in 3 ms, same pid. The window raise itself can't be seen on headless weston, and winit can't raise on Wayland (known limitation) |
| Second launch with `--status` | exit 0, "--status and --screenshot ignored: Bramley is already running" |
| `search …` | exit 4. `volume 500`: exit 2 |
| `PULSE_SINK` = a sink that doesn't exist | banner `Error("the audio output could not be opened")`, **no PipeWire stream at all**. MusicKit still reports Playing at 0.0. Before the fix, pipewire-pulse silently played to the default sink |
| Unguarded channel (socket path 169 bytes) | stderr shows fastframe-instance's WARN plus Bramley's WARNING line. The status carries `instanceGuarded: false` |
| Guard | `check-shipped-features.sh` exits 0 on release and 1 on `--features dev` (names app/dev) |

## Callers (S12/S13)

- Each ack is the status JSON. Correlate on `(engine, seq)`. Wait for `applied >= seq` and then assert the effect too (state, itemId, position), because `applied` advances for skipped commands as well.
- An exit-0 ack means the command was queued. A failed engine start can still drop it; the banner then says why.
- Exit codes: 0 ok, 1 channel error, late or unreadable reply, 2 usage, 3 not running, 4 not yet available, 5 refused.
- A command is applied only if the UI takes it before the client's deadline (3 s after sending). Otherwise the client exits 1 and it is never applied.
- The slot follows `XDG_RUNTIME_DIR`. Export it in agent and cron shells.
