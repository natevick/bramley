# Drift rule: data and proposal (2026-10-10, awaiting Nate)

R1 now always lands in zone 3 (RSS > 200), so the reference-drift rule decides every v1 memory verdict. The reference binary is old main, and its code never changes. Even so, its PSS has climbed during the day, while the app's lead over it has held.

| Run | Reference PSS (before / after) | R1 PSS | R1 USS | App − reference | Verdict |
|---|---|---|---|---|---|
| S2c run1 | 80.2 | 160.2 | 141.9 | 80.0 | PASS (zone 2) |
| S2c run2 | 79.5 | 161.0 | 142.7 | 81.5 | PASS (zone 2) |
| S2c r2-run1 | 83.5 / 83.7 | 165.1 | 142.8 | 81.5 | PASS (zone 2) |
| S2c r2-run2 | 83.7 / 83.5 | 164.2 | 141.8 | 80.6 | PASS (zone 2) |
| S3 s3-rebase-3 | 86.8 / 87.0 | 168.9 | 142.1 | 82.0 | PASS (zone 3) |
| S3 s3-s5-rebase | 87.0 / 86.9 | 169.8 | 143.0 | 82.9 | PASS (zone 3) |
| S4 s4 | 86.9 / 86.9 | 170.0 | 143.2 | 83.1 | PASS (zone 3) |
| S4 s4r3 | 88.5 / 88.4 | 171.3 | 144.3 | 82.9 | VOID (+5.2) |
| Norm norm-s4 | 88.2 / 88.6 | 170.3 | 143.4 | 81.9 | VOID (+5.3) |
| Norm norm-run1 (busy box) | 71.3 | 165.1 | 142.7 | 93.8 | VOID (−12.0) |

Units are MiB. The recorded reference value is 83.3.

## What the table shows

- **On a quiet box, the app moves with the reference.** The lead stays between 80.0 and 83.1, while the reference itself moved 9 MiB.
- **App USS is flat** at 141.8–144.3, so the app isn't growing. Shared-page accounting is what moves.
- **Cause of the climb: unexplained.**
  - The dpkg log shows no mesa, llvm, libdrm or glibc update today; weston was installed on 10-09.
  - The reference's RSS rose about 9.5 MiB while its USS stayed flat.
- **The one busy-box run went the other way.** The reference dropped 12, and the app's lead jumped to 93.8. Sharing deflated the reference's PSS more than the app's.

## Options. These apply to FUTURE counted runs only; existing VOIDs stay VOID.

1. **One-direction rule (recommended).**
   - Upward drift (less sharing) can only make the app's PSS read higher, so a PASS stands and a FAIL becomes VOID.
   - Downward drift (more sharing) can only flatter the app, so a PASS becomes VOID and a FAIL stands.
   - It holds up as the box keeps creeping, and the busy-box case is still caught.
2. **Re-record the reference** on today's quiet box. This is simple, but the creep will hit the bar again.
3. **Both.**
