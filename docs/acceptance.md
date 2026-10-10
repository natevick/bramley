# Bramley v1 (Linux) acceptance bar (pre-registered 2026-10-10)

Nate delegated the definition of "done" to Norm on 2026-10-10. Bramley v1 on Linux is done when `scripts/acceptance.sh` passes every check below **in two consecutive runs**, and Nate has seen that run's screenshots. These thresholds are fixed now, before any acceptance run. A threshold moves only with Nate's approval, recorded here with the date.

## How it runs

- **Environment:**
  - weston headless with `--renderer=gl` on the real GPU (`sg render`). The renderer string must not contain llvmpipe.
  - A temporary PipeWire null sink; the default sink must be unchanged.
  - Real Apple Music, with the developer token from the CI-published file (or `~/.config/bramley` before S4) and the user token from the keyring.
- **Driver:** the shipped `bramley` release binary, controlled only through its own CLI (S11) and MCP server (S12), the way a user or agent would. No test hooks in the shipped binary: `check-shipped-features.sh` must pass on it first.
- **Evidence:** each step writes a screenshot, a status snapshot and PSS/CPU samples to `target/acceptance/<run>/`. Screenshots with real album art go to Nate privately, never into the public repo.
- **Network-dependent timings:** the threshold applies to the median of 5 repetitions.
- **Memory validity** (procedure added 2026-10-10; no threshold changed): absolute PSS on the dev box drifted 10–20 MiB between sessions for the same binary. Every R1/R2/R3 run therefore also measures a fixed reference binary in the same session and reports USS and RSS next to PSS. If the reference has drifted more than 5 MiB from its recorded value, the run is VOID and gets re-run.

## Functional checks (end to end)

| # | Check | Pass |
|---|---|---|
| F1 | Cold start | First frame presented ≤ 500 ms after exec; window shows the library sidebar |
| F2 | Signed out | With no user token, the sign-in-needed banner shows and no playback starts |
| F3 | Search | `bramley search "bohemian rhapsody"` returns results ≤ 2 s; the UI shows them |
| F4 | Play | `bramley play <id>` gives audible, decrypted audio ≤ 3 s after the command; native decrypted seconds > 35 on a 355 s song |
| F5 | Controls | pause, seek 120, resume, next and prev each show in a snapshot ≤ 0.5 s, with a matching `applied` sequence number |
| F6 | Album playthrough | Plays the last 20 s of 3 consecutive album tracks: each auto-advances, and each track's drain is acknowledged before the next starts (no cut tail) |
| F7 | Library | Lists playlists; opens one with ≥ 100 tracks; scrolls to the end; covers render |
| F8 | Queue | `queue add` ×3 makes them play in order after the current track |
| F9 | MCP | A stdio MCP client lists the tools; `search` then `play` through MCP gives audible audio (same bar as F4) |
| F10 | Failure paths | A bad song id shows an error with no crash; with the network down, browsing shows an error with no crash, and recovery is automatic once it returns |

## Resource checks

| # | Check | Pass |
|---|---|---|
| R1 | Playing, with the library view showing ≥ 50 covers | Whole-process PSS median ≤ **200 MiB** over 60 s |
| R2 | Browsing, never played | PSS ≤ **100 MiB** (the playback engine starts lazily on first play) |
| R3 | 30-minute soak (continuous playback across ≥ 8 tracks, a cover-heavy view open) | PSS growth ≤ **10 MiB** between minute 5 and minute 30; no crash; no underruns logged |
| R4 | CPU while playing | ≤ **5 %** of one core, median |
| R5 | CPU while paused | ≤ **1 %** of one core, median |
| R6 | Binary size | Recorded, no bar |

## Did-it-run and sabotage controls (each must make the named checks fail)

- **No user token:** F4, F5, F6, F9 and R1 must fail; F2 must pass.
- **Corrupted developer token:** F3 and F4 must fail.
- **Ballast build** (`sabotage` feature only, so it can never ship): it allocates and touches enough memory to land 15 MiB over the R1 bar, sized from the same run's real R1 median. R1 must fail. The ballast binary also must fail `check-shipped-features.sh`.
- **Leak build** (`sabotage` feature): it leaks 3 MiB on every track change, which is ≥ 24 MiB over 8 tracks. R3 must fail.
- **llvmpipe forced:** the run is VOID.
- **No null sink or no PipeWire stream:** F4 is VOID, not PASS, but only once playback has started (position advancing). If playback never started, F4 is FAIL. Clarified 2026-10-10: S2a found the two rules collided, so a signed-out run came out VOID.
- **A missing artifact counts as a FAIL:** any check with no evidence file counts as FAIL, so a harness that ran nothing cannot pass.

## Out of scope for v1

Windows, macOS, music videos, lyrics, the EQ (pending Nate's compliance call), offline mode, and a disk artwork cache (Nate: memory only).
