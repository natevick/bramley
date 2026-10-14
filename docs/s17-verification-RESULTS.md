# S17 item-relative position: results (2026-10-14)

Pre-registration: `docs/s17-verification-PREREG.md`, plus Amendments 11–14 of `docs/s13-acceptance-PREREG.md`.

## Verdict
- The position fix works. The reported position no longer runs past the item's duration in any run since the fix. Before the fix, 547 of 716 soak samples did.
- **Not merged on these runs.** No counted run has PASSed at a commit that carries S17. The one counted run that wasn't voided or broken by a harness defect failed on a product bug that predates S17: S20, a transient storefront failure signing the user out.
- **Next.** S17 merges together with S20 once a counted run PASSes at the integrated commit. That commit is v0.1.2.

## Runs
| Run | Commit | Result | What it says |
|---|---|---|---|
| `s17-soak-30` | 611e5b3 | FAIL, as briefed | The brief asked for `--soak 30`, but the soak bar of 30 item changes needs the 120-minute run; 13 changes. 1 underflow and 1 readiness dip, 3 s after a seek. Position check PASS, max position − duration −2.2 s. |
| `s17-soak-120` | a08dca0 | **PASS** | 48 item changes, 0 stalls, 0 underflows, 0 dips. Max position − duration −0.19 s. Regated under A13: PASS, 0 seam samples. The 30-minute underflow did not recur in about 38 seeks. |
| `s17-counted-1` | a08dca0 | FAIL (F6) | A harness defect: the driver seeded F6's start from the flip sample's position (Amendment 12). R3 PASS. |
| `s17-counted-2` | abe64ed | VOID | A co-tenant: postgres JIT backends map `libLLVM` (Amendment 14). This is the second run voided this way, after `s15c-soak-120`. |
| `s17-counted-3` | 00f91a4 | FAIL (F4) | 1 of 5 cold starts came up SignInNeeded with the engine never started. This is S20, which predates S17. All other checks PASS: F6 3/3, R3 max overrun −3.22 s, 0 underflows, no co-tenant in 793 polls. |

## Can the check fail?
The new check, Amendment 11 with A13's seam rule, fails both old runs:
- `v1-2`: R3 FAIL with 125 of 180 Playing samples past the duration, worst +815.7 s.
- `s15c-soak-120b`: soak FAIL with 563 of 716, worst +815.9 s.

A13 predicted the counts "may drop". They rose instead (from 122 and 547), because a seam sample is now judged against the shorter previous duration. That is the conservative direction, so the prediction was wrong but the conclusion holds.

## Found along the way
1. **The seam skew (S18).** The item id and the position disagree for up to about 0.5 s at each track change, in both directions. That is visible in F6's 20 ms samples. The gate tolerates it (A11/A13). The product fix is a follow-up.
2. **S20.** `player.js` turned any storefront failure into SignInNeeded and the app killed the engine for the session. It was fixed on its own branch.
3. **The co-tenant rule (A14).** `libLLVM` alone no longer makes a GPU client, and RADV now does.
4. **A reference fail-fast.** `acceptance.sh` stops before taking any lock when the reference binary is missing.
