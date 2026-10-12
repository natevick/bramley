# S13 full acceptance harness: pre-registration (2026-10-11)

S13 makes `scripts/acceptance.sh` implement every check in `docs/acceptance.md`. The thresholds there are binding and do not move here. This file fixes the method, the repetitions, the verdict rules and the run plan before any counted run. Anything below that reads as a judgement call is marked **(deviation)**. It needs the coordinator's or Nate's approval before a PASS that depends on it counts toward "done".

## Files
- `scripts/acceptance.sh <run> [--llvmpipe | --controls]` does the following, in order:
  1. runs the pre-flight;
  2. builds the sabotage, dev and shipped binaries;
  3. runs `check-shipped-features.sh` on each;
  4. writes `build.json`, which records the commit, dirty state, mode, feature-check exits and the shipped binary's size in bytes;
  5. runs the drivers under `sg render`;
  6. prints the verdict.
- `scripts/acceptance/drive.py`: the live drivers. They write one evidence file per check into `target/acceptance/<run>/`.
- `scripts/acceptance/gate.py <dir>`: verdicts, a pure function of the evidence directory.
- `scripts/acceptance/test_gate.py`: covers every rule.
- `scripts/acceptance/mutants.py`: mutates `gate.py`. Every mutant must be killed, and its no-op control must survive.
- `scripts/app/covers-session.py` keeps only the shared primitives: weston and null-sink session, isolation, the guard, smaps.
- s7, s8 and s10, `mcp-session.py`, `run.sh` and `s2a_verdict.py` are removed; their checks now live here.

## Every session
- **Isolation:**
  - each session gets its own config, keyring service, cache and `XDG_RUNTIME_DIR`, which holds the instance slot;
  - `assert_isolated` refuses the real ones;
  - `realstate.json` (real token mtime and size, keyring items, cache) must be unchanged, or the gate is FAIL.
- **Before launch:** `bramley now` must exit 3, or the harness refuses.
- **Audio guard:**
  - it watches from launch to exit and SIGKILLs the app if any stream links to anything but the null sink;
  - before any play, the null sink must exist, the default sink must not be the null sink, and the guard must have made a clean poll. Otherwise the play is refused.
- **Pre-flight:** exit 2 when another weston, another bramley, or a non-standing GPU client is running.
- **Process control:** apps are killed by their own process group only.
- **Network down (F10):**
  - The shipped binary runs under `bwrap --unshare-net`, which leaves it only loopback. The host network is never touched.
  - "Network returns" is `slirp4netns` attached to that app's netns; the harness tracks its pid.
  - The pid for guard, memory and `now` checks is read from the app's status, never bwrap's.

## Drivers and pass rules
All timings are from just before the CLI command is executed until the change is observed in the `--status` file, polled every 2–10 ms. The `--status` file is shipped CLI surface, and it is read for observation only. Five repetitions everywhere `REPS` appears.

| # | Driver (shipped binary unless stated) | PASS when |
|---|---|---|
| F1 | 5 cold launches on the default view | **median** first frame ≤ 500 ms; every launch is on Home with the library sidebar listing ≥ 1 playlist |
| F2 | signed out: launch, `play 6781024437` | banner `SignInNeeded`; play refused (exit 5); never Playing, engine never started, no stream |
| F3 | `bramley search "bohemian rhapsody"` × 5 in one app | every exit 0 with ≥ 1 song; **median** wall ≤ 2 s; the Search view shows that search's results (view `seq` moved, `results` > 0) with **median** ≤ 2 s |
| F4 | `play 6781024437` × 5 | median audio start ≤ 3 s (Playing, `applied` ≥ `seq`, 0 < position < 3); then that song past 35 s with `sentMs` > 35 000 and duration ≥ 300 s **(deviation: proxy for "native decrypted seconds")** |
| F5 | 5 rounds of: fresh `play`, `queue add 6781079518`, then pause, seek 120, play (resume), next, prev | each command's snapshot has the reply's `engine`, `applied` ≥ its `seq`, and the effect (Paused; position 119.5–122; Playing; the queued id; the first id). **pause and resume: every rep** ≤ 0.5 s. **seek, next, prev: median** ≤ 0.5 s, because they hit the network. Every rep must match within 3 s. |
| F6 | `play` album track 3, `queue add` tracks 4–6, then for each of 3 transitions seek to duration − 20 (item-relative; positions are cumulative across gapless items) | per Nate's amendment: (a) drain acked and no timeouts, or (b) sent ≥ (duration − target) − 3 s, skipped ≤ 0.1 s and no stall > 1 s, all read through flip + 5 s; landing within 1 s |
| F7 | MCP `library_playlists`; the harness finds the largest library playlist through the API | MCP lists ≥ 1; the largest has ≥ 100 tracks; the **dev build** opened on it (`--route playlist:<id> --scroll-end 6`) reaches the end with covers **(deviation, below)** |
| F8 | `play` album track 10, `queue add` tracks 1, 7, 4 (not album order), seeking each to its end | the played order is exactly that, and every add exits 0 |
| F9 | stdio MCP client: `tools/list`, `search`, `play` × 5 | the 10 tools; search returns the song; F4's audio bars |
| F10 | (a) `play 1`, an unresolvable id; (b) launch offline, `search`, then bring the network back | (a) an error banner or refusal, app alive (same pid). (b) search exits non-zero and the Search view shows `error`, app alive; after the network returns, with **no command**, Home reaches 50 covers within 90 s, and then `search` succeeds and the view shows results. |
| R1, R2 | as S2c (Amendments 1–2) | ≤ 200 / ≤ 100 MiB with zones and the one-direction drift rule |
| R3 | `play` album track 1 + `queue add` the other 17. Each track longer than 165 s is sought to duration − 10 once it has played 150 s. Snapshot and smaps every 10 s for 30 min, Home (50 covers) shown. | growth = median(minute 29–30) − median(minute 5–6). **USS growth > 10 is FAIL (zone 1)**; otherwise PSS growth ≤ 10 decides, with the one-direction drift rule. No crash, restart or fork; ≥ 8 tracks; every sample Playing with 50 covers; no stall (position behind wall clock by > 2 s between samples of the same item with no seek between) **(deviation: underrun proxy)** |
| R4 / R5 | a separate app **launched without `--status`** **(deviation)**, playing / paused; `/proc/<pid>/stat` CPU over 30 × 2 s | median ≤ 5 % / ≤ 1 % of one core, and the state is right before and after |
| R6 | `stat` of the shipped binary | recorded |

### Zone 2 is not used for R3
For a difference, USS ≤ PSS ≤ RSS does not order the growths: shared pages can come and go. So RSS growth under the bar does not bound PSS growth, and R3 uses only zone 1 (USS) and zone 3 (PSS with drift).

### Audio validity for playing checks (F4, F5, F6, F8, F9, R3, R4)
Once a check has started playback, it is VOID if, inside its window:
- any guard poll shows a target other than the null sink; or
- no poll shows the null sink.

If playback never started, the check is FAIL, not VOID. That is acceptance.md's 2026-10-10 clarification.

## Verdict precedence (per check)
1. **Missing evidence is FAIL.** This covers the check's own files, its session's guard file, and the session files (`build.json`, `session.verdict.json`, `weston.log`). The memory checks also need both reference files. Missing evidence beats VOID.
2. **VOID** if any of these hold:
   - a renderer (weston's log line, the app's, or the reference's) lacks radeonsi or names llvmpipe;
   - the default sink changed;
   - the null sink was left behind;
   - the guard killed the app;
   - the audio-validity rule above;
   - the reference sha, pid or renderer;
   - drift, per the one-direction rule, on R1, R2 and R3.
3. **FAIL:** a dirty tree, the shipped feature check failing, or any rule in the table.
4. Otherwise PASS.

**Gate (counted run):**
- FAIL if the real state changed.
- Otherwise VOID if any check is VOID. This is unchanged from S2c, so a VOID beside a FAIL is VOID and is re-run.
- Otherwise FAIL if any check FAILs.
- Otherwise PASS if the ballast holds.

## Controls
- **Ballast** (every counted run): as S2c Amendment 1, with one change. The expected USS rise is the requested MiB **plus the RGBA cover copies the sabotage build keeps**, read from the same run's `decodedBytesKept` (about 1.76 MiB). The tolerance stays 3.0.
  - **Why:** S9's +3.3 against 3.0 was this fixed 1.8 MiB offset plus noise. Subtracting a measured constant proves the same thing without widening the band.
  - The ballast holds only if its R1 reasons are exactly one, the USS bar.
- **`--controls` run.** It has its own reference readings and the same session rules. It holds only if every one of these holds:
  - **No user token** (signed out): F4, F5, F6, F9 and R1 FAIL for a reason other than missing evidence, and F2 PASSes.
  - **Corrupted developer token:** the **dev build** with `--developer-token-url` serving a malformed token from a loopback fixture **(deviation)**.
    - **Why the dev build:** the shipped binary renews a rejected cached token from the CI URL, so the shipped path heals itself.
    - F3 and F4 must FAIL substantively.
  - **No stream:** `PULSE_SINK` names a sink that does not exist, so F4 never starts and is FAIL ("playback never started"). Real playback without the null sink is never created, because the fallback is the real speakers. The VOID-after-start branch is covered in `test_gate.py`.
  - **Leak:** the gate-sabotage binary with `BRAMLEY_GATE_LEAK_MIB=3` keeps 3 MiB per new item. It holds if all of the following are true:
    - R3 FAILs in zone 1;
    - there are ≥ 4 item changes after minute 5;
    - the binary fails the shipped-feature check.
  - **Missing artifact:** an empty evidence directory gives FAIL on every check.
- **`--llvmpipe` run:**
  - `LIBGL_ALWAYS_SOFTWARE=1` is set on the apps only.
  - It runs F1, R2, F4 and R1. All four must be VOID with a renderer reason, and llvmpipe must be seen in the shipped samples.

## Deviations to approve
1. **F7 open/scroll on the dev build.** The shipped CLI and MCP have no way to open a playlist or scroll it, and pagination advances only on scroll. Options:
   - accept the dev-build leg; or
   - add a shipped navigation command (an app story).
2. **F4/F9 "decrypted seconds"** uses `sentMs` > 35 s on a ≥ 300 s song. The decrypt counter and the R3 underrun counter live only behind the engine's `diagnostics` feature, which the shipped binary lacks. Exposing them in the shipped status would be an app change, not made here.
3. **R4/R5 without `--status`**, so the measurement isn't inflated by the harness's observation file.
4. **Corrupted developer token on the dev build.**
5. **Medians for F1 and F5's network commands**, and every-rep for pause and resume.

## Expected from the smoke (2026-10-11, S9b running in parallel; not counted)
Written down now so a counted result can't be retro-fitted:
- F1 ~100 ms; F3 wall ~0.25 s and view ~0.4 s; F4 and F9 audio ~0.8 s (first play ~2.2 s); F6 3/3 gapless; F8 in order.
- R4 ~4.5 %.
- **Likely FAIL as app findings:**
  - F5: seek and prev are acknowledged only after ~530 ms; pause, resume and next are fine.
  - F10: Home covers never reload after the network returns. The fetch is one-shot, and search alone recovers.
  - R5: ~2 % paused.
- If these FAIL in the counted runs, they are reported with evidence and routed, not fixed in this branch.

## Run plan
- Two consecutive counted runs (`s13-1`, `s13-2`) on a quiet box, from a clean tree at the commit that adds this file, or a later harness-only commit.
- Then `s13-controls` (`--controls`) and `s13-llvmpipe` (`--llvmpipe`).
- A VOID run is re-run and reported, never dropped.
- Real-art screenshots from the dev build go to `target/screens/PRIVATE-real-art/acceptance/<run>/`, never into the repo.
