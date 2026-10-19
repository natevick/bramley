# S29 disk cache: results (2026-10-18)

Judged against `docs/s29-cache-PREREG.md` (commit 839ba22), unchanged. 16 real launches, spawned 20:36:11 to 20:53:45 (first spawn 39 minutes after the PREREG commit), every gap at least 70.2 s, no playback. All 16 passed the did-it-run checks. Builds: new = natevick/s29-cache at 839ba22, main = 8d0e0c5, both `--release --features dev`.

Seconds from spawn. S = Songs, A = Album (one fixed large library album). `rows` = first list rows, `covers` = all visible covers (15 textures on every launch).

| arm | route | n | rows (each) | covers (each) | covers median |
|-----|-------|---|-------------|---------------|---------------|
| new cold | S | 3 | 2.21 2.21 2.81 | 2.51 2.51 3.21 | 2.51 |
| new warm | S | 3 | 0.2 0.2 0.2 | 0.2 0.2 0.2 | 0.20 |
| main cold | S | 2 | 2.21 2.41 | 2.61 2.81 | 2.71 |
| new cold | A | 3 | 1.60 1.81 2.01 | 1.71 1.91 2.11 | 1.91 |
| new warm | A | 3 | 0.2 0.2 0.2 | 0.2 0.2 0.2 | 0.20 |
| main cold | A | 2 | 1.60 1.81 | 2.01 2.21 | 2.11 |

**Verdict: ship.**
1. Warm covers over cold covers: 0.08 on Songs, 0.105 on Album. The bar is 0.50 on both.
2. New cold covers over main cold covers: 0.93 on Songs, 0.91 on Album. The bar is 1.20 on both. The gap is inside the spread (0.2 to 0.7 s), so the cache costs the first visit nothing measurable.

**Did it run.** Cold launches showed `apiHits` 0 and `apiMisses` 2. Warm launches showed `apiHits` 2, `apiRevalidations` 2 and `artHits` 65 with no misses. Main launches carry no cache block.

**Limits.**
- Main ran at n=2 per route, not 3: n=3 on all arms needs 18 launches against the cap of 16 (see the PREREG). Bar 2 is judged on that.
- The status file resolves to about 0.1 s, so a warm 0.2 s means "in the second reading". Warm covers are art-cache hits decoded locally.
- Warm launches had `developer-token.json` deleted, so they still paid a developer-token fetch before the cached list could be served. A normal warm launch skips it.
- A warm launch revalidates in the background; these runs show the refetch ran, not a changed library. The changed-data path is covered by unit tests only.
- Sign-out, eviction at the 512 MB cap and the keyring wipe ordering were exercised in unit tests, not against Apple.

**Size.** After a launch the cache held 0.25 to 0.27 MB of art and 1.75 MB of API responses per route, inside the isolated session. The harness deletes it with the session; `~/.cache/bramley` was never created.
