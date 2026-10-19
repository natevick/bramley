# S29 disk cache: pre-registration (2026-10-18)

Committed before any real measurement launch. Thresholds do not move after data comes in.

**Question.** Does the on-disk cache (decisions.md, 2026-10-18) make a repeat visit load faster without slowing a first visit? S24 found the list request (1.8 to 2.9 s) dominates and covers follow it by 0.1 to 0.45 s, so the API cache should matter more than the art cache.

**Builds.** New = natevick/s29-cache. Main = the base 8d0e0c5 (natevick/s28-storefront-403). Both `cargo build --release --features dev`, same toolchain.

**Routes.** `songs`, and `album:<id>` for one fixed large library album (id in scratch only, never committed).

**Arms.** `Nc` new build, empty cache dir. `Nw` new build, second launch reusing that cache dir. `M` base build, cold. Every launch runs through `cs.session` + `cs.isolated` (own XDG_CACHE_HOME, config and keyring service, null sink, no playback). Between a cold and its warm launch only `developer-token.json` is deleted, so warm fetches the developer token like cold.

**Budget conflict.** n >= 3 for all three arms on both routes needs 18 launches; the cap is 16. This plan is new cold n=3, new warm n=3, main n=2 per route (16). Main n=2 is the weak arm. Two more launches (S-M3, A-M3) complete n=3 only if the lead authorizes them; they are not run otherwise.

**Order** (S songs, A album; spawns >= 70 s apart; a different launch sits between each cold and its warm):
S-Nc1 S-M1 S-Nw1 A-Nc1 A-M1 A-Nw1 S-Nc2 S-M2 S-Nw2 A-Nc2 A-M2 A-Nw2 S-Nc3 A-Nc3 S-Nw3 A-Nw3

**Metrics** (seconds from spawn; the status file is read every 0.1 s and rewritten on change).
- rows: first reading with `songs.loaded` or `tracks.loaded` > 0. Reported, not judged.
- covers: time of the last change of `library.artTextures` before a 6 s window with no change; cap 90 s. Primary.

**Ship bar, fixed now.** Medians per arm per route (main n=2: the median is the mean).
1. warm covers <= 50% of cold covers, on BOTH routes.
2. new cold covers <= 120% of main cold covers, on both routes.
Otherwise do not ship; RESULTS reports what dominates. Within-arm spread (max minus min) is reported; a gap inside it is noise.

**Did-it-run.** A launch counts only if all hold:
- it reached its route (`route` equals the title), painted a frame, loaded the list, and had `artTextures` > 0 at the plateau;
- the sink guard did not kill it;
- new cold shows `apiHits` = 0 and `apiMisses` >= 1; new warm shows `apiHits` >= 1 and `artHits` >= 1; main has no `cache` block.

An invalid launch is not repeated (no spare budget) and is excluded. Any arm left with n < 2 makes the verdict inconclusive.

**Named false positives.** OS page cache and DNS warmth: the arms interleave, and no warm launch directly follows its own cold one. Apple network bursts (S22): a gap inside the within-arm spread is noise. Mock runs (`--mock`, local server) are harness debugging only and never count.

**Harness.** `scripts/app/s29-cache.py`; its ledger records every spawn time.
