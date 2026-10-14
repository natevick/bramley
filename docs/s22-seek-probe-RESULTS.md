# S22 slow-seek probe: results

## s22-probe-1 (2026-10-14, commit 8204437): INCONCLUSIVE

Did it run: commit 8204437, clean tree, mode probe, real state unchanged, leak grep clean, 6 license actions (one replay), 71 polls in every arm, control arm C landed in 0.6 s.

| Arm | Origin | t_skip | t_app |
|---|---|---|---|
| A, LONG from about 150 s | 151.8 | 0.4 | 0.4 |
| B, LONG from about 295 s | 297.0 | 0.4 | 0.4 |
| C, SHORT control | 151.8 | 0.6 | 0.6 |

Arm A landed fast, so it does not count, and by the PREREG the verdict is INCONCLUSIVE. The probe did not exercise the slow path.

## Why the design missed it

Item labels: LONG is 500.79 s, MID is 355.15 s, and PRED is the 139.88 s item that comes before LONG in the acceptance playlist.

I tabulated every R3 seek in the post-S17 runs (s17-counted-1, s17-counted-2, s17-counted-3, s17-soak-30, s17-soak-120, v012-counted-1): 77 seeks, with landing measured from item-relative position. The script was scratchpad seek_table.py, which is not committed.

| Item | How it was entered | Origin | Gap to target | Landing | n |
|---|---|---|---|---|---|
| LONG | seamless from PRED | about 143 | about 347 | 4.57 to 5.06 s | 4 of 4 |
| LONG | seamless from PRED | about 287 | about 204 | 0.25 to 0.5 s | 5 of 5 |
| LONG | fresh, single-song queue (probe-1) | 151.8 | 339 | 0.4 s | 1 of 1 |
| MID | seamless | about 147 | about 198 | 1.71 to 1.96 s | 6 of 6 |
| a 243.86 s item | seamless | 144.7 | 89 | 3.81 s | 1 of about 7 |

All other seeks landed in at most 0.5 s.

LONG at the same gap was slow 4 of 4 times when it was entered seamlessly from PRED, and fast 1 of 1 times when it was entered fresh. The single-song design left out the entry path, and maybe the presence of a next item as well. Gap alone does not explain the table: MID at a gap of about 198 s is consistently about 1.75 s, while LONG at a gap of about 204 s is fast.

## Deviation from the decision table

The PREREG said to re-run an INCONCLUSIVE once. I did not repeat the identical run. A and B both landed in 0.4 s, so the miss is in the design, not noise, and an identical re-run would most likely spend 6 license actions to repeat 0.4 s. The pre-registered re-run is folded into probe v2 as its fresh-entry control arm. v2 copies R3's context: LONG sits mid-queue after PRED and is entered by a natural transition. It runs on a trace build with the leaks below fixed, so a reproduction also shows what `seekToTime` waits on. v2 is pre-registered in its own file before it runs. Stop rule: if v2 also fails to reproduce, blind probing stops, and the work moves to reading the trace from an R3-shaped run.

The trace build's leaks, found while building probe-1:
- `host()` keeps the query string or userinfo of a URL that has no path;
- the CDM trace prints key ids.
