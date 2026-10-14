# S17 verification runs: pre-registration (2026-10-14)

Written and committed before either run. Both runs use this commit, from a clean tree. They decide whether S17 (item-relative position) merges and ships as v0.1.1.

## The runs
1. **`scripts/acceptance.sh s17-counted-1`**, counted mode.
   - **Pass:** the gate PASSes under `docs/acceptance.md` and `docs/s13-acceptance-PREREG.md` Amendments 4–11.
   - This is the first live run of F5, F6 and F8 with item-relative positions.
2. **`scripts/acceptance.sh s17-soak-120 --soak 120`**.
   - **Pass:** the pass condition of `docs/s15c-long-soak-PREREG.md`, unchanged, plus Amendment 11. That condition is: no stall, ≥ 30 item changes, `outputUnderflows` Δ 0, `readinessDips` Δ 0, one pid, pacing ≤ 20, and the run completes.

## Why the soak
- **What happened:** S17's 30-minute soak (`s17-soak-30`, at 611e5b3) had 1 output underflow and 1 readiness dip on the same sample. It came 3 s after a seek to duration − 10, on item …4431, a seek that rebases.
- **The baseline:** the pre-S17 runs (v1-1 to v1-4 and both S15c soaks) had 0 underflows in about 112 seeks.
- **No known mechanism:** S17 changes only the snapshot read path and adds a field to buffered frames.
- **What a clean run would show:** a 120-minute soak makes about 38 seeks. If S17 had raised the per-seek rate to 1 in 9, the chance of 0 underflows in 38 seeks is about 1 %. A clean run therefore argues against such a rise.

## Decision rule
- **Both PASS:** S17 merges. The 30-minute underflow is recorded as unattributed.
- **The soak shows any underflow or dip:**
  - S17 does not merge yet.
  - A 120-minute soak runs on main 0353fd8 (pre-S17) as the control.
  - If the control shows one too, the hazard pre-dates S17: it becomes its own story, and S17 merges.
  - If the control is clean, S17 stays blocked and is investigated.
- **The counted run FAILs:** S17 does not merge until the failing check is understood.

## Named false positives
- **Host load:** other sessions on this host may load the CPU. The load average is sampled every 10 s outside the harness. An underflow while load1 exceeds the CPU count is reported as load, not attributed to S17.
- **The network or Apple:** a readiness dip next to a failed license or segment fetch in the run log is reported as network.
- **A VOID under the existing rules** (co-tenant, guard kill or renderer): re-run it; don't judge it.

## Did it run
Each verdict must show:
- `commit` = this commit, with `dirty` false;
- the expected `mode`.

The soak must also have:
- ≥ 700 snapshots;
- ≥ 30 seeks.
