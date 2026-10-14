# S15c long soak: results (2026-10-14)

Pre-registration: `docs/s15c-long-soak-PREREG.md`. Mode: `scripts/acceptance.sh <run> --soak 120`. Information only: it never counts toward v1. Both runs were at 874ebe0 from a clean tree, holding the GPU lock, with a unique null sink and the routing guard.

## Verdict: PASS (`s15c-soak-120b`). Race 3 does not reopen.

| Condition | Result |
|---|---|
| No permanent stall (> 30 s frozen in Loading or Playing) | 0 driver stalls, 0 frozen snapshot runs |
| ≥ 30 item changes | **51** (23:38:16 to 01:37:56), with 3 queue refills at 00:09:56, 00:53:25 and 01:36:52 |
| `outputUnderflows` Δ | 0 |
| `readinessDips` Δ | 0 |
| No crash or restart | one pid, 1 process in all 716 snapshots |
| Pacing peak ≤ 20 | 5, with 0 waits |
| Run completed | last snapshot at minute 120.05 |
| Banners | 0 snapshots |

The reference readings were 88.6 and 88.4 MiB PSS (recorded value 88.6). The co-tenant watch saw nothing in 1,403 polls, the leak grep was clean with both plants caught (275 library names, 19 files), and the real token, keyring and cache were unchanged.

## PSS growth (reported, no bar)
| Minute | PSS | USS | RSS |
|---|---|---|---|
| 5 | 178.3 | 151.5 | 211.0 |
| 30 | 184.9 | 158.1 | 217.6 |
| 60 | 186.8 | 159.9 | 219.4 |
| 90 | 187.8 | 160.9 | 220.4 |
| 119 | 189.4 | 162.5 | 222.0 |

- **Growth from minute 5 to the end:** PSS +11.1, USS +11.0, RSS +11.1 MiB, or about 5.8 MiB/h averaged.
- **The rate falls:** +6.6 MiB came in the first 25 minutes (about 16 MiB/h), then +4.5 MiB over the next 89 minutes (about 3 MiB/h). That fits a warm-up followed by a slow climb, and it can't be separated from a slow leak in one run.
- **For scale:** R3's bar is 10 MiB between minute 5 and minute 30, and this run was +6.6 MiB over the same window.

## The first soak run (`s15c-soak-120`) is void
- Its soak rules reported no failure: 0 stalls, 51 changes, both counters at 0, one pid, and a pacing peak of 5. The same numbers appear as in the passing run.
- It did not count, for two reasons that had nothing to do with the soak:
  - **Missing reference binary:** my worktree had no `target/reference/`, an untracked file, so both reference readings were missing, and missing evidence is a FAIL. The worktree needs the file copied from another worktree (sha prefix `1a72eb4b2880ac5a`).
  - **Co-tenant sighting:** short-lived postgres backends that map libLLVM (query JIT) tripped the watch at 21:43–21:44, four polls over about 50 s. The watch flags any new pid mapping a GPU library, whatever its name. Under the existing rule that is a VOID.
- The second run saw no such process.

## What this does and doesn't show
It shows no stall, underflow or readiness dip over 51 item changes and 2 hours. It doesn't prove the HTTP-cache fix's residual of ≤ 1.6 % per duplicated preload is zero: 51 changes at that rate would still leave a roughly 1-in-2 chance of a stall in a run of this size if the residual were real, so one clean run is encouraging but not conclusive. A second clean run would raise the confidence.
