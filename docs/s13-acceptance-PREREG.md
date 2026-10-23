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

## Amendment 1 (2026-10-11, after s13-1, s13-2, s13-controls and s13-llvmpipe; before any further counted run)
Procedure only; no bar moves. The four runs at e13ba5b are reported and scored under the rules above. None of them is re-scored.

1. **Co-tenant watch.**
   - **Evidence:** S9b's own gate started at 19:47:3x, while my s13-2 was passing its pre-flight. Its builds run before its weston appears, so the start-only check missed it, and s13-2's reference read −13.8 MiB with shipped USS 26 MiB low. A non-gated S9b live session also ended at 19:37, inside s13-1's soak.
   - **Change:** the pre-flight condition is now polled every 5 s for the whole run, excluding the harness's own process tree.
   - **Rules:**
     - any sighting VOIDs every check of that run;
     - a missing record, or one with zero polls, is FAIL.
2. **Leak grep.**
   - **Evidence:**
     - in s13-1 and s13-2, the 4 repo "hits" were one library playlist name that is an ordinary English word, found in prose and code;
     - `main..HEAD` also counted main's newer TASKS.md commit as added lines;
     - in the controls and llvmpipe runs, the grep had no library names, so its plant could not fire.
   - **Change:**
     - the repo side now uses the merge-base (`main...HEAD`);
     - library names are fetched by the grep itself through the documented API, in memory, in every mode;
     - library names that already occur in main's tracked text are not counted in the repo grep, because such a name can't be told from prose. They still count in the run-file grep.
3. **`gate.py --preflight` is the pre-flight entry point again.** Other worktrees' scripts call it.

Re-run plan unchanged: two consecutive counted runs, then `--controls` and `--llvmpipe`, all at the commit that adds this amendment.

## Amendment 2 (2026-10-11 late, coordinator's instruction; no rule changes)
The branch is rebased onto main at 8c6e77a, where S8, S9b and S10 are merged, so the counted runs exercise the final feature set.
- s13-1 to s13-4, s13-controls and s13-llvmpipe ran on the pre-S9b base. They are reported but do not count.
- The counted set is `final-1` and `final-2` (consecutive), `final-controls` and `final-llvmpipe`, all at the commit that adds this amendment.
- The coordinator confirmed no other worktree will use weston or the GPU during these runs. The co-tenant watch still applies.

## Amendment 3 (2026-10-12, after final-1; procedure only)
- **Evidence:** final-1 was VOIDed by a single sighting, at 00:32:02, of a `bramley` process that mapped no GPU library. s13-4 had the same one-poll signature at 23:08:26. A short-lived CLI process that exits between the watch reading its `comm` and walking its parent chain fails the "ours" test and is flagged. This could not be reproduced on demand: 285 polls under 4 continuous CLI spawners gave 0 sightings.
- **Change:** a sighting whose process no longer exists at the end of its poll is dropped. An exited process holds no memory. A real co-tenant lives long enough to be seen on the next poll, 5 s later.
- **Status of the earlier runs:** final-1 stays VOID as scored. The counted set is now `final-2` and `final-3` (consecutive), plus `final-controls` and `final-llvmpipe`, at the commit adding this amendment.

## Amendment 4 (2026-10-12, review fix round; fixed before any further counted run)
The review (`scratch/review-s13.md`) showed rules that could PASS a v1 missing the bar. acceptance.md's bars are unchanged. Everything below is method, and every number is fixed now.

**Safety**
- `acceptance.sh` holds an exclusive `flock` on `/run/user/<uid>/bramley-gpu-session.lock` (shared by every worktree) and refuses if it is held. `covers-session.session()` takes the same lock unless its parent holds it.
- Null-sink names carry the run, the pid and a random suffix.
- The pre-play routing check runs inside the `play`/`queue` CLI wrapper and before every MCP play, so no path skips it.
- On abnormal exit, a trap removes this run's token copies and its `bramley-gate-<run>-*` keyring items.

**Engine counts.** The shipped status now carries `decryptedMs` (decrypted media time), `underruns` (media-element starvation) and `decryptFails`. They are counts only; no audio reaches the harness.

**F1:** every launch ≤ 500 ms, not the median.

**F4: judged on cold plays.**
- 5 fresh app launches (same isolated config), one `play` each.
- **Audio start:** the snapshot shows `applied ≥ seq`, the asked item and Playing, and the position advancing by more than **0.05 s** between two snapshots. "0 < position" is no longer used.
- Median ≤ 3 s, and every `play` exits 0.
- Each launch's play is judged from the command to position > 37 s. **All of these must hold:**
  - decrypted delta ≥ **35,000 ms**;
  - `skippedMs` delta ≤ **100 ms**;
  - `decryptFails` delta = **0**;
  - |position moved − wall clock| ≤ **1.0 s**, measured from the first audible snapshot to the end;
  - at the end: Playing the asked item, with no banner;
  - catalog duration ≥ 300 s.
- The five warm replays are kept as information only (`f4warm.json`).

**F9:** the same audio-start and judged-play rules apply to its last play, and every MCP play must return no error.

**F5: judged on the effect, with `applied ≥ seq` (and a matching `engine`) as a second condition.**
- **pause:** Paused, and the position unchanged for **0.3 s**. The time is when that unchanged position was first seen.
- **seek 120 (while paused):** |position − 120| ≤ **0.1 s**.
- **resume:** Playing, and the position advancing by more than **0.05 s** between two snapshots, from ≥ 119.9.
- **next / prev:** the item changes within the bar, and the new item's position advances (> 0.05 s) with a median ≤ **3,000 ms**.
- **Bars:** pause and resume are every rep ≤ 500 ms; seek, next and prev are the median of 5 ≤ 500 ms. Every command must exit 0, and both `applied` and the effect must appear within 5 s.
- **Known limit:** the engine snapshots every 250 ms, so an effect that needs two snapshots can read up to ~250–500 ms late. This can only add FAILs.

**F6: computed in gate.py from raw counters.** drive.py records the counters at three points: the applied seek (base), the id flip, and the first snapshot at or after flip + 5 s (end).
- **Branch (a):** read at the end: `drainsAcked` up ≥ 1 and `drainTimeouts` unchanged.
- **Branch (b):**
  - sent **base→flip** (outgoing track only) ≥ (duration − target) − **3 s**, so an early contiguous cut can't pass;
  - skipped **base→end** ≤ **0.1 s**;
  - no position stall > **1 s** within flip ± 5 s.
- **Landing:** |landing − target| ≤ **1 s**.
- The window must reach flip + 5 s.

**R3**
- `underruns` delta over the soak = **0**.
- **Pacing:** between consecutive samples (~10 s apart) of the same Playing item, the position may not fall behind the wall clock by more than **1.0 s**. Only pairs with a harness seek inside [first sample − 1 s, second sample + 1 s] are excused, using recorded seek timestamps.
- **Evidence:** a 3 s SIGSTOP left `underruns` at 0 (the counter tracks media-element starvation, not output starvation) but lagged the position by 2.87 s. Pacing is the check that sees it.

**R4 / R5**
- 30 bins of **10 s** (5 minutes): the median is judged and the mean is reported.
- Ticks include `cutime`/`cstime`.
- Every bin must have exactly 1 process.

**Co-tenant watch:** the standing GPU clients are recorded **by pid** at the start. Any other pid that maps GPU libraries is flagged, whatever its name.

**Leak grep**
- `runFiles` > 0 is required.
- **The self-test is a real file:** a needle is written under the run directory, and the same file walk must count exactly one more hit.
- **Needles also include:** the largest library playlist's track names, and the album's track titles (F6, F8 and R3). They are held in memory only.
- The committed docs are grepped again after the final RESULTS commit.
- **Matching:** needles match on word boundaries.
  - A name that also occurs as a whole word in the tracked tree, at main or HEAD, is not counted on either side. Such a name can't be told apart from our own code and prose.
  - The number excluded is recorded.
  - **Evidence:** in a smoke run, four plain-substring hits were ordinary words inside "worktrees", "planes", a CDM log word and a JSON key.

**Gate**
- Any phase error in `errors.json` is FAIL. The only exception is a phase skipped after a guard kill, which stays VOID through the guard rule.
- realstate also records the real config directory listing, which covers the developer token.

**Controls: each holds only on its stated reason.**

| Control | Required |
|---|---|
| No user token | F2 PASS; F4 and F9 "playback never started" with every F4 rep's banner `SignInNeeded`; F5 "0 rounds"; F6 "0 of 3 transitions"; R1 "playback never started" |
| Corrupted developer token | **New fixture:** a well-formed JWT, the real header and payload with the signature reversed, served from loopback to the dev build. F3 "did not return songs", with every rep's error containing "developer token was rejected" (Apple's 401). F4 "playback never started" with every rep's banner `SignInNeeded` |
| No stream | F4 "playback never started", with every rep's banner containing "the audio output could not be opened" |
| Cut tail | F6 "2 of 3 transitions"; exactly one cut transition failing with no drain, skipped > 0.1 s, and a landing within 1 s |
| Leak | R3 FAIL with "USS growth" (zone 1 only); ≥ 4 changes after minute 5; the binary fails the shipped-feature check |
| **Stall (new)** | SIGSTOP the playing app for 3 s, then compare snapshots 10 s apart on the same item: the R3 pacing rule must report a lag > 1.0 s |
| **Guard kill (new)** | a **second, existing** null sink (unique name, confirmed present and not the default before play) is the app's `PULSE_SINK`, while the guard allows only the run's null sink. All of the following hold: the guard kills the app ("stream linked to" the second sink); the app is gone; the default sink is unchanged before and after |
| Missing artifact | every check FAILs on an empty directory, and its first reason is missing evidence |

**Probe results (smoke, not counted, 2026-10-12)**
- F5: pause 19–30 ms; seek 521–781 ms; resume 277–282 ms; next 299–555 ms (advancing 552–808); prev 549 ms.
- Cold F4: 2.0 s, with 38.7 s decrypted per play and a pace error of 0.05 s.
- R4: 3.9 %. R5: 1.8 %.
- The guard-kill and stall controls fired.
- **App finding:** with a rejected developer token, the playback banner reads `SignInNeeded`; the developer-rejected banner is outranked in `app.rs` `banner()`. The control records that as the stated reason, and it is routed as an app finding.

**Counted set:** two consecutive runs (`fix-1`, `fix-2`), then `fix-controls`, all at the commit adding this amendment. A VOID is re-run and reported.

## Amendment 5 (2026-10-12, after fix-1; procedure only)
fix-1 (at 5fae888) is reported under Amendment 4's rules. It does not count, because the F9 rule below changed.

1. **F4/F9 skipped is read from the first audible snapshot to the end, not from the command.**
   - **Evidence:** fix-1's F9 judged play read 1,513 ms skipped. It was 5,948 ms both at the first audible snapshot and at the end: all of it was queued audio of the *previous* MCP replay, flushed by this play. Nothing was skipped while the judged play was audible.
   - The decrypted and decrypt-fail deltas are still read from the command.
2. **Queue commands record their reply's `seq` and `applied`** (F6, F8, R3). This is evidence only; no rule reads it.
   - **Why:** in fix-1, F8 saw only its first item, and R3 played nothing for 30 minutes. R3's `play` and 17 `queue add`s all exited 0, while `seq`/`applied` stayed at 85/85.
   - A smoke repro of F6 → F8 → R3 passed, so the cause is unknown and intermittent. It is reported as an open finding.
3. **F5 caveat (from S14b).** The engine's `position` counts frames handed to PulseAudio, not frames heard, so "advancing" can register up to ~200 ms before sound. This is a proxy limit:
   - resume measures ~280 ms, so +200 ms stays under the 0.5 s bar;
   - next/prev audibility is judged against 3 s.
4. **F5 definitions, reconciled with S14b's probe.** Amendment 4's definitions stand for this harness; they were registered before any counted run. Where S14b's probe differs:
   - **seek:** this harness accepts |position − 120| ≤ 0.1 s, against 0.01 s in S14b's, and does not separately require Paused. `seek` runs while paused, and pause is judged just before it.
   - **pause:** both time it when the unchanged position is first seen, confirmed by a 0.3 s hold.
   - **resume and next/prev:** identical, apart from the 0.05 s step. Both require `applied ≥ seq`.
5. **Lock path:** `/run/user/<uid>/bramley-gpu-session.lock` (`flock`, exclusive). acceptance.sh and covers-session's `session()` hold it for a run's lifetime. Other harnesses may hold it around their own sessions; acceptance.sh then refuses with exit 2.

**Counted set:** `fix-2` and `fix-3` (consecutive), then `fix-controls`, at the commit adding this amendment.

## Amendment 6 (2026-10-12, after fix-controls; procedure only)
- **Evidence:** fix-controls' leak grep fetched 0 library names. The real user token began getting HTTP 403 from `/v1/me/storefront` and the library API between ~16:47 (the leak soak was still playing) and 16:49 (the grep). The fetch failed inside a suppressed block, and the plant self-test passed on a token fragment. So that run's "clean" grep checked tokens only.
- **Rule:** the leak grep is clean only if it held **> 0 library names**. A grep that couldn't fetch names now fails the gate.
- **Correction (2026-10-12, verification):** the leak soak was not playing at ~16:47; it had frozen at 16:35:42. So the start of the token refusal is not bracketed by playback evidence.
- **Amendment 5 was a loosening applied after fix-1,** which this file had named counted. It is disclosed as a deviation pending approval in RESULTS, with the evidence.
- **Status of the runs:**
  - fix-2 and fix-3 had 275 names each, so their verdicts are unchanged.
  - Re-scored, fix-controls' gate is FAIL on the blind grep, although every control held.
  - Re-running the controls needs a valid user token. The real token file is unchanged since 2026-10-09, yet Apple now refuses it. A fresh sign-in by Nate is needed first.

## Amendment 7 (2026-10-12, S14c; written after fix-1, fix-2, fix-3 and fix-controls, before any code for it and before any further run)
**This amendment is made after seeing data.** Every change below was prompted by fix-1 to fix-controls. It is stated plainly so a reader can weigh it. None of those runs is re-scored: their evidence has no `outputUnderflows`, no in-launch ballast window and the old banner.

**1. R3 gates output starvation, not readiness dips (measurement validity).**
- **Evidence:** the engine's `underruns` counter counts the media element falling below `HAVE_FUTURE_DATA` while potentially playing. fix-2 and fix-3 read 51 and 47 per soak, on specific tracks, with pacing lag ≤ 0.2 s. In fix-controls the 3 s SIGSTOP, a real output starvation, left it at 0; only pacing caught it. A counter that is blind to the one starvation we caused on purpose cannot stand for "no underruns logged". This corrects what R3 measures; it is not a retro-fit to make a FAIL pass. If anything it can add FAILs, because the new counter sees starvation the old one missed.
- **New counter:** the engine counts PulseAudio `UNDERFLOW` commands the server sends for our playback stream. Verified against source, not memory: command 63 (`PA_COMMAND_UNDERFLOW`, `src/pulsecore/native-common.h`; `COMMAND_UNDERFLOW` in pipewire-pulse's `commands.h` at tag 1.6.2, the version installed here), payload `u32` stream channel, then an `s64` read offset when the protocol version is ≥ 23 (`protocol-native.c` `PLAYBACK_STREAM_MESSAGE_UNDERFLOW`; pipewire's `stream_send_underflow`). Every server underflow is counted; nothing is excused in the engine. Status key: `outputUnderflows`.
- **Old counter:** same semantics, renamed `readinessDips` in the status. It is reported, never gated. (The engine's dev-only diagnostics status keeps its old `underruns` key.)
- **New R3 rule:** `outputUnderflows` (last sample − first sample) = **0**, **and** the Amendment 4 pacing rule (no lag > 1.0 s between consecutive same-item Playing samples outside a seek window). Both must hold. The counter must be readable in the first and the last sample; otherwise R3 is FAIL ("outputUnderflows unreadable"), as the old rule did. `readinessDips` delta is recorded in the verdict for information.
- **Named false positives, fixed before any data:**
  - **Drain.** pipewire-pulse 1.6.2 sends one `UNDERFLOW` when a stream that has drained sits empty (`do_process_done` after `draining` clears). R3 in fix-2 and fix-3 had 0 drains (gapless album), so none is expected.
  - **Seek on stock PulseAudio.** PulseAudio's flush does not mark the stream underrun, so a seek can report one `UNDERFLOW`. pipewire-pulse's `stream_flush` sets `is_underrun = true` without sending, and cork and stream creation do the same, so on this box seeks, pauses and new streams should report none. The harness runs on pipewire-pulse 1.6.2.
  - An `UNDERFLOW` that clusters at seeks or drains in a live run is **investigated, not excused**. Excusing it needs another amendment.
  - **Unmeasured:** whether readiness dips cause real underflows. If R3 now FAILs on `outputUnderflows`, that is a finding about the dips, not a reason to drop the rule.
- **Stall control:** it holds if the SIGSTOP pair shows `outputUnderflows` Δ ≥ 1 **and/or** a pacing lag > 1.0 s. Both are recorded. A missing counter never counts as firing. Expected live reason: **"output underflow and/or pacing lag"**, and we expect both, because the stream's buffer target is 200 ms and the app is stopped for 3 s.

**2. The bad-developer-token control expects the developer banner.**
- S14c makes the developer-token-rejected banner outrank `SignInNeeded` in `app.rs` `banner()`.
- **Rule:** the control's F4 holds only if every rep's banner contains "developer token was rejected" (it was `SignInNeeded`, which could not be told from the no-user-token control). F3's rule is unchanged.
- **Known dependency:** the app learns the developer token is rejected from a cover or catalog request getting HTTP 401. If a rep's play fails before that 401 lands, its banner can still read `SignInNeeded`; that is an app finding, not excused.

**3. The ballast is measured inside one launch.**
- **Evidence:** across runs the ballast's rise residual (measured − (requested + kept covers)) spanned −3.3 to +0.9 MiB, and fix-1's −3.3 missed the 3.0 tolerance. The rise was the ballast launch's USS minus a different launch's (shipped) USS, so launch-to-launch variance sat inside it.
- **Change:** the gate-sabotage build allocates nothing at start. It allocates and touches the ballast once, when the file named by `BRAMLEY_GATE_BALLAST_FILE` appears, holding the MiB to allocate, and reports the touched MiB in the status as `ballastMiB`.
- **Procedure in the ballast session (same binary, same process):**
  1. play the R1 song and take the **pre** window: `R1_SAMPLES` samples at 1 s, as R1;
  2. write the trigger; wait until the status reports `ballastMiB` equal to the size;
  3. take the **post** window the same way. The post window is the one R1's rules judge (zone 1 required, as before).
- **Size:** unchanged: `ceil(200 + 15 − shipped R1 USS median)`, from the same run's shipped R1.
- **Rise:** median post USS − median pre USS. **Expected:** the size + (kept covers at the end of post − kept covers at the end of pre). Both windows hold the same kept covers, so the old fixed +1.76 MiB term goes away. **Tolerance stays 3.0 MiB.**
- **Also required:** every sample in both windows has the same pid; a missing or empty pre window, or a `ballastMiB` that never matched, means the ballast does not hold.
- The landing band, the zone-1-only rule and the shipped-feature check are unchanged. The trigger and the status key exist only behind `gate-sabotage`, whose `bramley-unshipped-feature:app/gate-sabotage` marker stays.

**Live checks this amendment still owes** (the real user token is refused by Apple, HTTP 403; no signed-in run is possible until Nate signs in again):
- the stall control firing on `outputUnderflows` (and pacing);
- R3's `outputUnderflows` and `readinessDips` on main, after S14b;
- the in-launch ballast residual;
- the bad-token control's banner.

**Counted set:** a new pair of consecutive counted runs plus a `--controls` run, at the commit that implements this amendment or a later one.

## Amendment 7a (2026-10-12, S14c review round; supersedes Amendment 7 §1 and tightens §3; before any run)
The coordinator decided this after the S14c review. Amendment 7 stays as written above; where they differ, this amendment wins. `docs/acceptance.md` is unchanged.

**Withdrawn from §1:** Amendment 7 made `readinessDips` information-only. That demoted the clause that FAILed both counted runs (fix-2 Δ 51, fix-3 Δ 47), so it was a loosening as well as a correction. The sentence "If anything it can add FAILs" was wrong and is withdrawn.

**R3 (replaces §1's rule).** R3 gates all three of the following, and each must hold:
- `outputUnderflows` Δ (last sample − first) = **0**: the new starvation counter;
- `readinessDips` Δ = **0**: Amendment 4's "underruns = 0", renamed, with the same semantics;
- the Amendment 4 pacing rule: no lag > **1.0 s**.

A counter missing from the first or the last sample is FAIL. This adds a real starvation counter and drops nothing.

**Expectation, stated before any run:** `readinessDips` should fall on main, because S14b's e11055a counts sink-queued audio toward readiness. If live runs still show dips with no pacing lag, the coordinator takes the data to Nate. No rule changes here.

**Stall control (replaces §1's bullet).** It holds only if **both** of these hold for the SIGSTOP pair:
- `outputUnderflows` Δ ≥ **1**;
- a pacing lag > **1.0 s**.

A missing counter is not ≥ 1. This tightens the control before any live data. Under Amendment 7's "and/or", a dead counter, such as one pinned at 0, still held the control on pacing alone, and R3 would then read Δ 0 forever.

**Why pacing stays required: a named blind spot.** pipewire-pulse 1.6.2 marks a stream as underrun, without sending `UNDERFLOW`, in three cases:
- at creation (`pulse-server.c` ~L1790);
- after a flush (`stream.c` `stream_flush`);
- on cork (~L2696).

It also skips the check while corked. So starvation that begins right after a new stream, a seek or an uncork sends no `UNDERFLOW`. Examples: a new item that never starts, or a slow refill after a seek. Only pacing sees those. Pacing in turn excuses pairs within ±1 s of a harness seek.

**Underflow evidence.** Whenever R3's 1 s pacing poll sees `outputUnderflows` rise, the soak records:
- wall time;
- item;
- position;
- the new count;
- `drainsAcked`;
- the time of the last harness seek.

A FAIL can then be attributed to a seek, a drain or mid-track starvation without another run. This is evidence only; no rule reads it.

**Ballast (tightens §3).** Same-launch is proven only if both windows meet these rules:
- **pid:** every sample in both windows carries the session's recorded pid. A sample with no pid fails.
- **Pre-ballast window:** it also gets R1's per-sample rules:
  - `R1_SAMPLES` samples;
  - every one Playing with no banner;
  - ≥ 50 covers;
  - exactly 1 process.

## Amendment 8 (2026-10-13, leak grep; fixed before the rule changes)
Two defects showed up in v1-1 on main (3106814): `leakgrep.json` had `plantedCaught` false, and 15 names counted as "in base".

**1. The self-test split by scope.**
- **Defect:** on main, `git diff main...HEAD` and `git log main..HEAD` are empty. The old self-test also required that repo text be non-empty, so it could never pass on main.
- **Run plant (unchanged):** a needle file under the run directory must raise the run count by exactly one.
- **Repo plant:** counting the private needles over `repo + "\n" + needle` must give `hits.repo + 1`. No non-empty diff is required.
- `leakgrep.json` records `plantedRun` and `plantedRepo`. **The grep is clean only if both are true,** along with the existing rules: 0 hits, `runFiles` > 0 and `libraryNames` > 0.

**2. Names already in the tree are no longer exempt by default.**
- **Defect:** every name found anywhere in the tree was excluded, so a name that had already leaked into main could never be flagged.
- **New rule:** a name is excluded only if one of these holds:
  - it is one word and appears in `/usr/share/dict/words` (case-insensitive);
  - the SHA-256 of its lowercased form is listed in `scripts/acceptance/leak-allowlist.sha256`. Each line holds the hash and a kind (`catalog-constant`, `ordinary-phrase` or `test-constant`), and never the name.
- **Every other name found as a whole word in the tracked tree at HEAD is a repo hit,** counted once per matching line. This is in addition to the diff and commit-message hits.
- **Limit:** a hash of a short common phrase can be brute-forced from a dictionary. The allowlist keeps names out of plain sight; it is not a secret.

**3. Classification at 3106814 (no names recorded).**
- The live fetch found 14 in-tree names, 13 library and 1 catalog. v1-1 counted 15; the library set may have changed since.
- **None is a real leak:**
  - 12 are single dictionary words used in code or prose: HTTP header values, identifiers, comments, and words like "running" or "transitions".
  - 1 is a two-word ordinary phrase used once in compliance.md prose: allowlisted as `ordinary-phrase`.
  - 1 is the catalog song title that acceptance.md names as the search term: allowlisted as `catalog-constant`.

## Amendment 9 (2026-10-13, S13c: play pacing; fixed before any code for it and before any further run)
Procedure only. No threshold of any check changes.

**Why.** Apple's license server refused plays after about 30 license requests in about 8 minutes on one account. It answered HTTP 200 with a JSON error (status −1002, failureType 3062) instead of a license. The refusals continued for about 7 minutes even at wide spacing, then stopped once recent volume dropped (`docs/s14d-cold-play-RESULTS.md`). A run that plays faster than that measures Apple's throttle, not Bramley.

**License-requesting actions.** Each of these counts as one:
- every command that can start a new item: `play <id>` (setQueue), `next`, `prev`, a jump, and every MCP `play`;
- every harness seek that is meant to end an item: the F6 and F8 seeks near the end;
- every item change the R3 soak observes.

Signed-out and no-stream plays are counted too, even though they never reach the license server. This is conservative: it can only add waits.

**The rule: at most 20 actions in any rolling 10-minute window.**
- Before a counted command or seek, the harness sleeps until the action fits: until fewer than 20 actions fall in the last 600 s.
- The count is per account, so it carries across runs. It is kept in `/run/user/<uid>/bramley-license-actions.json`, next to the GPU lock, which serialises runs.
- **R3:** before its first `play`, the soak waits for room for 6 actions (the play plus about 5 changes in its first 10 minutes). After that it records each item change it observes and never waits. A wait inside the soak would let an item end naturally, and it would distort the soak.

**Timing is unchanged.**
- A wait always happens before the timed command starts. Each command's start time is taken after the wait, so no wait is ever inside a timed measurement.
- Waits don't change what a check measures. Only the gaps between commands get longer.

**Evidence.** `pacing.json` records:
- the limit and the window;
- the prior actions (from the shared file, within the window before the run started);
- every action, with its time and kind;
- every wait, with its time, its length in seconds, and the count in the window when it started.

**Gate.**
- The verdict reports the number of waits, their total seconds, and the peak count of actions in any 600 s window (prior and this run's together).
- A missing `pacing.json` is FAIL.
- A peak above **20** is FAIL, as a harness defect, in every mode.
- A window counts an action at time `u` for a window ending at `t` when `t − 600 < u ≤ t`, the same boundary the pacer uses.
- **Clarification (written with the code; no rule change):** a wait ends 1 ms past the boundary. A wait that ended exactly on the boundary could round to zero, and the pacer would then make no progress.

**Run length.** The estimate is reported with the S13c hand-back. Nothing is cut here: any cut to plays is proposed for the coordinator to decide.

## Amendment 11 (2026-10-14, S17: item-relative position; fixed before any code for it and before any run)
A new check on the `position` the app reports. No threshold of any other check changes.

**Why.** The status `position` was the media element clock. MusicKit's seamless transitions append the next item to the same SourceBuffer at `timestampOffset = endOfBufferTime`, so that clock runs on across items, and a seek with no transition pending carries the offset (MusicKit sets `currentTime = t + offset`). Two recorded runs show it: in the 2 h soak `s15c-soak-120b`, 547 of 716 Playing samples have position > duration + 1, up to +815.9 s; in v1's counted R3 (`v1-2`), 122 of 180, up to +815.7 s. The UI clamps it, so the status file, CLI `now` and MCP `now_playing` carried a raw value that no check looked at.

**The check.** In counted R3 and in `--soak` mode, on every sample whose state is Playing: `position <= duration + 1` (seconds). Any violation is a FAIL, with the count, the worst excess and the item in the reason. A Playing sample with no position or no duration is a FAIL too (missing evidence). In `--soak` mode it joins the pass condition of `docs/s15c-long-soak-PREREG.md`; a soak run still never counts toward v1.

**Scope: v1 stays done.**
- v1's recorded verdict stands. It is not voided and not reopened by this amendment.
- The check applies to runs made at or after this commit.
- `gate.py` does not look at commit ancestry. Gating an older run directory with it is a probe of the check, not a re-judgement of that run.

**One consequence, stated now.** The app resets the reported position when the audio crosses the seam, which can be up to one timeupdate (about 250 ms) before MusicKit's `itemId` flips. A 10 s snapshot can land in that window with the old `itemId` and a position near 0. The pacing check (`lags`) compares positions of consecutive samples of one item, so it would read that reset as a stall. It therefore skips a pair only when the position fell and the wall clock between the two samples carried the earlier position to within 1 s of the item's duration. Every other pair is judged as before.

**Not changed.** Seek input was already item-relative. `HTMLMediaElement.currentTime`, as MusicKit sees it, is unchanged; only the reported snapshot position is item-relative.

## Amendment 12 (2026-10-14, S17: the F6 driver under item-relative positions; fixed before the re-run)
No threshold changes.

**What happened.** `s17-counted-1` (a08dca0) FAILed F6 with "2 of 3 transitions passed". The third transition never started.
- The driver seeded each transition's `start` from the flip sample's position.
- Under the carried clock that was the offset of the new item. Under item-relative positions it is about 0, unless the flip sample lands in the seam window.
- Transition 2's flip sample read 172.6, the old item's time with the new `itemId`. The next transition then waited for a position above 173.6 on a fresh item and timed out after 40 s.
- This is a harness defect: the check did not run.

**The fix.** `start` is 0 for every F6 transition, because every position is item-relative. `landedOff` is therefore `base.position − target`, the same quantity it measured before. The next transition starts at least `F6_WINDOW_S` (5 s) after the previous flip, which is well past the seam window.

**Amendment 11's seam note, corrected from evidence.** The F6 samples, polled every 20 ms, show the seam window in both directions:
- the position reset about 0.5 s before the `itemId` flipped (transition 1);
- the `itemId` flipped about 0.5 s before the position reset (transition 2).

The window is up to about 0.5 s, not 250 ms. The `lags` carve-out of Amendment 11 is unchanged.

**What counts.** `s17-counted-1` stays a FAIL on record. The decision of `docs/s17-verification-PREREG.md` uses the re-run `s17-counted-2` at the commit that carries this amendment and the fix, with the same pass rule.

## Amendment 13 (2026-10-14 07:2x, S17: the other direction of the seam; committed before s17-counted-2 ends and before any of its evidence was read)
No threshold changes.

**The defect.** Amendment 12 recorded that the seam skews both ways. Amendment 11's carve-out handles only one of them: the position resets before the `itemId` flips. In the other direction the `itemId` flips first, so for up to about 0.5 s a sample carries the new `itemId` and the new duration but the old item's position. A 10 s R3 or soak snapshot that lands there fails a correct build in one of two ways:
- If the new item is shorter than the old position by more than 1 s, `position_overruns` reports it.
- If the new item is more than about 11 s longer, `seam_reset` reads the new duration, so it does not excuse the pair, and `lags` reports a stall of about the old position.

The reviewer confirmed both shapes against the real gate functions. The recorded runs have no such sample in 70 item changes, which is a rate of up to about 4 % per change at 95 %.

**The rule.** A *seam sample* is the first Playing sample at a new `itemId` whose position continues the previous sample's clock:
- the previous sample is Playing;
- `prev.position + (t − prev.t)` is within `PACE_S` of its position;
- `prev.position + (t − prev.t) >= prev.duration − SEAM_S`.

Seam samples are handled like this:
- `position_overruns` judges a seam sample against the previous item's duration, not its own, so the overrun check still runs on it.
- `lags` skips a pair whose first sample is a seam sample.

**Can it still fail?** Under the original bug, the first sample of every new item continues the old clock, which looks like a seam sample. The regates of `v1-2` and `s15c-soak-120b` must therefore still FAIL on position. The count may drop, but not to zero, because most of their overrun samples are not the first at their item.

**How the decision runs are judged.**
- `s17-counted-2` was started at abe64ed, before this amendment.
  - If it PASSes under the gate it ran with, a regate of a copy under this amendment may confirm the PASS.
  - A regate cannot rescue a FAIL. If it FAILs only on a seam sample, the decision run becomes `s17-counted-3`, run at the commit that carries this amendment's code.
- The PASS of `s17-soak-120` is regated on a copy under this amendment. A FAIL there would mean re-running the soak.

## Amendment 14 (2026-10-14, the co-tenant watch: libLLVM alone is not a GPU client; written before s17-counted-3)
**What happened.** Two runs were voided by postgres backends whose only match on the GPU library list was `libLLVM`, which postgres's query JIT maps:
- `s15c-soak-120`, at 21:43;
- `s17-counted-2` (abe64ed), with 4 polls from 1791987558.8.

`libLLVM` is a general compiler library. It is not a GPU driver.

**The rule.** The watch, the preflight and the standing-client snapshot count a process as a GPU client when its comm is `weston` or `bramley`, or when it maps one of:
- `libgallium` or `radeonsi`, the Mesa GL drivers;
- `libvulkan_radeon` (RADV), which is new and closes a gap.

A process whose only match is `libLLVM` is not a GPU client.

**Nothing escapes.** Every Mesa GL client also maps `libgallium` or `radeonsi`, so dropping `libLLVM` alone hides none of them.

**What counts.** `s17-counted-2` stays VOID on record. Its other readings are not judged. The decision run of `docs/s17-verification-PREREG.md` is `s17-counted-3`, run at the commit that carries Amendments 13 and 14 in code.

## Amendment 15 (2026-10-15, S22: the seek is a hang guard, not a pacing probe; written before v013-counted-1)
**Evidence.** `docs/s22-seek-probe-RESULTS.md` on `natevick/s22-seek-probe`. Post-seek landing time in R3 is dominated by the network path to Apple: range fetches of 0.5–1.2 s in bursty minutes, with every Apple endpoint slowing together. `v012-counted-1` failed R3 on `[1.06]`, the pacing lag of the first sampled pair after a slow seek, while playback itself was fine.

**(a) The pacing rule.** `lags()` excludes every pair whose first sample is less than 15.0 s after an R3 seek. Before, it excluded only seeks within ±1 s. A seek that falls inside the pair still excludes it, as before.

**(b) A new R3 check, seek landing.** For each R3 seek, on the item that was playing at the seek:
- the target is `round(duration − 10, 1)`, which is what `drive.py` seeks to, so the gate rebuilds it from the item's duration in the last sample at or before the seek;
- the landing is `(t − seek) − (position − target)`, taken at the first post-seek sample whose position exceeds the target by more than 0.05 s;
- the check FAILs if any landing is over 15.0 s, or if a seek never lands before its item changes or the samples end.

This is a hang guard, not a performance target.
- The bar is the pacing window, `SEEK_SETTLE_S`. A landing that window cannot absorb is a FAIL, so the two rules describe one thing.
- A hang shows up as a seek that never lands, or as position playing on to the target, which is a landing of minutes.
- The observed range is 0.2–5.1 s over 77 R3 seeks in six post-S17 runs (`s17-counted-1` to `-3`, `s17-soak-30`, `s17-soak-120`, `v012-counted-1`).
- The first draft of this amendment set the bar at 6.0 s. It was raised to 15.0 s before any v0.1.3 data existed. 6.0 s sat 0.94 s above one outlier caused by the network, so an Apple network burst could fail a release run, which is the failure this amendment exists to stop.
- Typical seek time is not gated. Every landing is recorded in the R3 output as `seekLandingsS`, for reading across runs.

R3 stays one of the 16 checks.

**Scope.** It applies to runs made at or after this commit.
- `v012-counted-1` stays FAIL on record. Replaying its `r3.json` through the new rules gives R3 PASS with a max landing of 5.06 s over 8 seeks. That replay is a probe of the rule, not a re-judgement of that run.
- No threshold of any other check changes.

## Amendment 16 (2026-10-16, v0.1.4: a session that does not start with shuffle and repeat off is VOID; written before v014-counted-1)
**Why.** `natevick/player-controls` adds shuffle and repeat to the player, and the status JSON carries `shuffleMode` (on/off) and `repeatMode` (off/all/one). The harness assumes items play in order and advance, so a session that starts with either mode on cannot be judged. `docs/acceptance.md` already lists this under the VOID conditions.

**The rule, as `drive.py` and `gate.py` implement it.**
- `drive.py` records `modes` for each session: `shuffleMode` and `repeatMode` from the first status snapshot returned by any of the session's `App.until` waits that carries both fields, whether its predicate was met or it timed out. `App.close` writes `modes` to `<session>.guard.json`. A session that never sees both fields writes `modes` as null.
- `gate.py` compares `modes` with `OFF_MODES`, `{shuffleMode: off, repeatMode: off}`. A mode that is on (shuffle on, repeat all or one) is VOID. The reason names the session and the recorded modes.
- No `modes` key, `modes` null, or a null field is missing evidence, so the check is FAIL. A session whose status was never read is a broken start, and a VOID there would hide a real FAIL, because VOID outranks FAIL.
- Revised before any v0.1.4 run: the first draft recorded modes from the first `until` even on a timeout and made a null VOID, which a code read showed would turn a dead start into a VOID.
- The rule runs in every place a session's guard is read. That is `guard_voids` for each session named in a check's evidence (`sessions`, or `session`), which covers the soak and every check whose evidence names a session, and `memory_check` for the session behind R1 and R2. A VOID applies to every check that session feeds.
- `F1` now records its `sessions`, so its guard is read.
- The pinned reference binary predates both fields, and its records are not checked.

**F9.** The expected `tools/list` is now the 14 tools of `gate.TOOLS`, which adds `shuffle` and `repeat`. A server that lists any other set FAILs F9.

**Scope.** It applies to runs made at or after this commit. No threshold of any other check changes.

## Amendment 17 (2026-10-21, S34: a ballast that only misses its landing band, with the app's footprint intact, is VOID; written before any run that uses it)
**Why.** The ballast must land R1's USS median at 215 ± 5 MiB (`R1_BAR + BALLAST_OVER`, `LANDING_BAND`). Twice the host measured high, and the ballast added its MiB correctly but landed outside the band. `holds` was false and the gate FAILed with all 16 checks PASS.
- `v042-counted-1`: the ballast landed at 220.1 MiB, 0.1 MiB outside. Every other clause held. See `docs/v0.4.2-RESULTS.md`.
- `v060-counted-2`: the ballast's USS rose by 58.0 MiB, against 58.8 in `v060-counted-1`, but its launch started from about 205 MiB and landed at 262.7. See `docs/v0.6.0-RESULTS.md`.
- **Why not every band miss.** The ballast launch's pre-ballast USS is the app's own memory in a second launch, so a real regression could also land the control outside the band. RSS tells the two apart. In `v060-counted-1` the ballast launch read RSS 270.3, USS 215.4 (RSS-USS 54.9) and R1 read RSS 213.1. In `v060-counted-2` it read RSS 271.9, USS 262.7 (RSS-USS 9.2) and R1 read RSS 212.2. The footprint held and about 46 MiB of normally shared pages counted as private in that launch. Total footprint (RSS) is the guard: growth in the app raises it, and a sharing collapse does not. The guarantee is that a VOID needs the footprint within 10 MiB of R1's **and** the pre-ballast USS, corrected for the lost sharing, at or under the 200 MiB bar.

**The rule, as `gate.py` implements it.** The ballast clauses are `thresholdOnly`, `failsShippedCheck`, `ussRoseByBallast`, `oneLaunch`, `preWindowValid`, `touched` and `landedWithinBand`.
- If `landedWithinBand` is the only clause that is false, **and** the ballast launch's RSS median minus its measured USS rise is within `FOOTPRINT_BAND` (10 MiB, inclusive, either direction) of the shipped R1 RSS median, the gate is VOID. `ballast.bandMiss` names the miss, the pre-ballast USS and the RSS-USS split.
- `adjusted` is the pre-ballast USS minus `max(0, shipped split - ballast split)`, where split is RSS minus USS. The VOID also requires `adjusted` ≤ `R1_BAR` (200 MiB, inclusive). `v060-counted-2`: 204.7 - (54.5 - 9.2) = 159.4, so VOID. `bandMiss` carries `adjusted`.
- If the footprint differs by more than 10 MiB, `adjusted` is over the bar, or the shipped R1 RSS or USS is missing, the gate is FAIL and `ballast.footprintMiss` names the numbers, for example `ballast launch footprint 261.9 MiB RSS vs shipped 212.2 (>10)`.
- If any other clause is false (the USS did not rise, not touched, more than one launch, an invalid pre-window, R1 failed for more than the USS bar or did not fail, or the ballast passed `check-shipped-features.sh`), the control is broken and the gate is FAIL.
- The order is unchanged: FAIL if the real state changed or the run is unsafe, then VOID if any check is VOID, then FAIL if any check FAILs. The band-miss VOID is reached only when every check PASSed. Sabotage and llvmpipe modes are unchanged.

**Known limit.** Private growth that exactly matches the evicted shared pages keeps both the footprint and `adjusted` in range, and a band-miss VOID can then hide it. That needs a coincidence of two independent quantities, and a repeat on the re-run goes to Nate under the stop rule.

**Stop rule.** Superseded by Amendment 19, which has one rule for every VOID. A host that measures high would otherwise loop runs and spend Apple traffic on each.

**Scope.** It applies to runs started at or after commit 3491774. `v042-counted-1` and `v060-counted-2` stay FAIL on record, and Nate's decisions on them stand. No threshold of any check or of the ballast changes.

## Amendment 18 (2026-10-22, S46: a check that lost its evidence to a failed harness Apple call is VOID; written before any run that uses it)
**Why.** `v070-counted-2` FAILed on four checks. The harness's own Apple call in `scripts/app/s9-session.py` (`api()`) got HTTP 403 and aborted the browse phase. F3, F7, F9 and F10 never got their evidence, so each was FAIL on "missing evidence". Every phase that ran passed, and `v070-counted-1` at the same commit PASSed all 16 checks. A failure in the harness says nothing about the app, so the run cannot be judged. This is the same reasoning as Amendment 17.

**The rule, as `s9-session.py`, `drive.py` and `gate.py` implement it.**
- `api()` treats 401 and 403 as possibly transient. It makes 3 calls, sleeping 2 s and then 5 s between them, inside its existing bound of 6 iterations. Other 4xx, 429 and 5xx behave as before.
- If it still fails, it raises `HarnessApiError(code, path_kind)`. `path_kind` is the endpoint shape, such as `library/albums`, with no ids and no names.
- `drive.py` records a phase that died on `HarnessApiError` in `errors.json` as `{"kind": "harness-api", "code": 403, "path": "library/albums"}`. Every other phase error stays a string.
- A check is VOID, with the reason `harness Apple call failed: HTTP 403 on library/albums`, when all of these hold: its only problem is missing evidence files, and the phase that writes each missing file died on a harness-api error. `EVIDENCE_PHASE` is the map: `browse` writes `f3`, `f7`, `f9` and `f10-badid`, and it is the only phase that calls Apple directly.
- Missing evidence for any other reason is FAIL: no phase error, an app-originated phase error, a phase that did not die this way, a missing session record, or another failing rule in the same check.
- A real FAIL never hides behind this VOID. A FAIL in another check, a ballast FAIL or an unsafe run still FAILs the gate. Other VOIDs keep their precedence over FAIL, as in Amendment 17.
- In a counted run, a harness-api error with every check PASS is VOID, never PASS. In soak, controls and llvmpipe modes a harness-api error is a phase error and FAILs, as before.

**Known limits.**
- F10 reads `f10-badid` from browse and `f10-net` from offline. If only `f10-badid` is missing, `f10-net` is still judged. A real FAIL in it makes F10 FAIL, and a clean `f10-net` leaves F10 VOID.
- `drive.py`'s leak grep calls Apple inside `suppress(Exception)`. A 403 there still ends as a leak-grep FAIL, not a VOID.

**Stop rule.** Superseded by Amendment 19, which has one rule for every VOID.

**Scope.** It applies to runs started at or after commit 512ba41. `v070-counted-2` stays FAIL on record, and Nate's decision on it stands. Its archived `errors.json` is the old string, which the gate does not read as harness-originated. No threshold of any check changes.

## Amendment 19 (2026-10-22, S48: an R3 PASS over a host memory reclaim is VOID, and the ballast lands against its own baseline; written before any run that uses it)
**Why.** Norm's S45 investigation of the ballast sharing collapse (a memo outside the repo) traced both Amendment 17 band misses to the same cause. A host page reclaim during R3 unmapped shared library pages, and later launches mapped them as private. USS rose by about 45 MiB while RSS stayed flat. It also found a second miss. v0.7.0's ballast launch keeps about 8 MB of decoded covers that R1 does not, so it missed the band by +7 and +9 MiB with sharing intact. Nate approved both fixes on 2026-10-22.

**(a) R3 reclaim guard.**
- The drop is the largest fall of R3's own RSS from an earlier sample to a later one, taken over the soak samples. Recovery after the fall does not undo it.
- If the drop is 30 MiB or more (`RECLAIM_MIB`) and R3 has no FAIL, R3 is VOID. The reason names the drop.
- A FAIL stays FAIL, so a leak is never hidden by a reclaim.
- `v070-counted-1` replayed through this rule: R3 RSS fell 72.1 MiB, so its R3 PASS is VOID. Its growth was -24.8 MiB, which shows why a PASS over a reclaim proves little.

**(b) The ballast lands against its own baseline.**
- The target is the ballast launch's pre-window USS plus the ballast size, within the same 5 MiB band. It was 215 ± 5, which assumed the ballast launch starts where shipped R1 does.
- The ballast is still sized from shipped R1. `ussRoseByBallast` and every other clause are unchanged, and R1 must still FAIL on its USS bar alone.
- `v070-counted-2`: pre-ballast USS 161.3, ballast 63 MiB, rise 63.0, landed 224.3. The target is 224.3, so the ballast holds. Under 215 ± 5 it missed by 9.3. `v070-counted-1`: ballast 58 MiB, landed 265.4, holds the same way.
- **Amendment 17 after this change.** The rise must match the ballast size plus any growth in kept covers within 3 MiB. The landing must match the ballast size within 5 MiB. So a miss now needs kept covers to grow by more than 2 MiB between the two windows. A sharing collapse no longer causes a miss, and R3's guard voids the run that had it. The band-miss VOID stays as it was and is rarely reached now.
  - The footprint and adjusted-USS guards now run for every intact ballast, not only on a band miss, and `holds` needs them. Judged against its own baseline, a ballast launch whose footprint has grown lands on its own target, so only these guards catch it. Three cases FAIL: the footprint 48 MiB over shipped R1's, a pre-ballast USS of 230 over the bar, and an adjusted USS over the bar.
  - Both v070 ballasts still hold. Footprint gaps are 6.8 MiB (`v070-counted-1`) and 8.8 MiB (`v070-counted-2`), against the 10 MiB band. Adjusted USS is 164.0 and 161.2.

**(c) Diagnostics, no judging change.** Every memory sample in `drive.py` now carries `vmstat` (the `pgscan*` and `pgsteal*` counters from `/proc/vmstat`) and `split` (shared and private MiB for each sampled process). The gate does not read them. They are there so the next collapse can be attributed.

**Not changed.** The S45 memo's open question stays open: which process is the sharing partner, and what drove the reclaim. The 8 MB of kept covers is now absorbed by the target, not explained.

**Stop rule (replaces the per-cause rules in Amendments 17 and 18).** Two consecutive counted runs of the same release that both end VOID, for any cause or mix of causes, stop the re-runs, and the decision goes to Nate. A PASS or a FAIL in between resets the count. This is stricter than the per-cause rules: a band-miss VOID followed by a harness-api VOID, or by an R3 reclaim VOID, now stops the re-runs too.

**Scope.** It applies to runs started at or after commit 512ba41. `v070-counted-1` and `v070-counted-2` stay as recorded. No threshold of any other check changes.

## Amendment 20 (2026-10-23, S52: R3 growth is judged on Anonymous; approved by Nate; written before any calibration run)
**Why.** Norm's S52 investigation (a memo outside the repo). The reclaim that voided `v071-counted-3` (-58.0 MiB RSS) and `-4` (-74.2) was file-only: `pgsteal_anon` was 0 in every drop interval and 9 and 22 MiB over 30 minutes, against 61 and 46 GiB of file steal. The harness cannot stop it (a `MemoryLow=` scope lost 43-96 of 192 MiB under real global reclaim, 0 of 3 runs protected). File reclaim also moves PSS and USS down, so a PASS over a reclaim can hide a leak: `v070-counted-1` passed with USS growth -24.8 while RSS fell 68.2.

**Rule.** R3 growth, the median of minute 5 against the median of minute 30 (same windows as before), is judged on `anon`: smaps_rollup `Anonymous` summed over the whole process tree. The bar is unchanged: growth > **10 MiB** FAILs, with the reason `Anonymous growth <x> > 10.0`. USS and PSS growth are still recorded and no longer fail R3.
- **19a becomes a diagnostic when `anon` is present.** `reclaimMiB` is recorded and no longer VOIDs.
- **Fail closed.** If any R3 sample lacks a numeric `anon`, R3 is VOID with `R3 samples lack anon, which Amendment 20 judges R3 growth on`; it cannot PASS. Amendment 19a (RSS fall >= 30 MiB is VOID) and the USS/PSS FAIL rules then apply as before, and a FAIL stays FAIL.
- **The reference drift rule does not apply to an anon-judged R3 (Norm's call, not part of the approval; Nate may override).** It exists because shared pages flatter PSS; `Anonymous` is not shared, and applying it would VOID an anon FAIL whenever the reference drifts up, hiding a leak. Every calibration result records both reference drifts so the effect of this choice is visible. Every other R3 condition, and the reference-missing VOIDs, are unchanged.
- **The leak control** (`controls.leak.holds`) now needs the R3 FAIL reason `Anonymous growth` instead of `USS growth`.

**Scope.** It applies to a run whose `build.json` carries `"amendment": 20`, which `scripts/acceptance.sh` writes from this commit. A directory without it is judged as before: the four archived `v071-counted-*` directories have no `anon` and no flag, and their `verdict.json` is byte-identical under the new gate. The sampler has recorded `anon` since commit f8d9166.

**Calibration, uncounted (`s52-cal-N`, never `-counted-`), pass conditions fixed before any run.** The v0.7.1 binaries of `v071-counted-4` (`bramley` sha256 prefix 2fc7234cac368a2f, `bramley-sabotage` 9a43c1b136e3b15a), each run under `flock /run/user/1000/bramley-gpu-session.lock`, the null sink, the pacer's 20 licence actions per 10 minutes, and `gate.py --host-calm` passing first. Each run is R3 only: a 30-minute soak plus the reference before and after. The verdict is `gate.check(out, 'R3', prefix)` on the build with `"amendment": 20`.
- **a) quiet baseline (`s52-cal-1`, shipped build, no hog).** Pass: every sample has `anon`; R3 PASS; `anonGrowth` <= 10; tracks played >= 8.
- **b) reclaim (`s52-cal-2`, same build).** The DIAG hog (a loop over 21 GiB of read-only on-disk files, in its own process group) starts when the soak clock starts plus 12 minutes (just after the queue is built) and stops after **45 GiB** read or 120 s, whichever comes first, chosen now; 45 GiB is the volume the DIAG experiment used. The start is checked afterwards against the sample minutes. Pass: R3 PASS with no VOID; `anonGrowth` <= 10; `reclaimMiB` >= 30; `pgsteal_file` advanced >= 10 GiB between the samples at minutes 12 and 15; tracks >= 8. If `reclaimMiB` < 30 the run is inconclusive, not a pass, and is repeated once with a 90 GiB cap and 300 s. A FAIL for a reason other than `Anonymous growth` (for example underflows from the hog's IO) is also inconclusive and repeated once with a 20 GiB cap.
- **c) leak (`s52-cal-3`, `bramley-sabotage` with `BRAMLEY_GATE_LEAK_MIB=3`, no hog).** Pass: R3 FAIL with `Anonymous growth` in the reasons, `anonGrowth` > 10, at least 8 track changes. Without it (a) and (b) prove nothing.
- **Did it run, every run:** `anon` in all samples (numeric), `len(changes)` in the R3 evidence >= 8 (not the gate's distinct-track count), both reference drifts and `reclaimMiB` recorded, the R3 file not empty, a verdict printed by the new gate. A run that violates any of these is not evidence.
- **Replay:** the old and the new `gate.py` give byte-identical `verdict.json` on all four `v071-counted-*` directories.

**Amendment 20 calibration results (2026-10-23; the gate of the Amendment 20 commit; evidence in `target/acceptance/s52-cal-N/`, local, with the runner copied to `cal/` in each; `s52-cal-smoke` was a 7-minute plumbing check and is not evidence).** Every run: the v0.7.1 binaries of `v071-counted-4`, null sink, flock held, `--host-calm` passed first, `anon` numeric in all 180 samples, no phase errors.

| Run | What | R3 | anon growth (min 5 → 30) | USS growth | `reclaimMiB` | track changes (after min 5) | reference drifts |
|---|---|---|---|---|---|---|---|
| `s52-cal-1` | a) quiet, shipped build | **PASS** | +5.4 (113.0 → 118.4) | +0.7 | 6.0 | 11 (8) | +0.6, +0.4 |
| `s52-cal-2` | b) 45 GiB hog at soak minute 12.0 | PASS, **inconclusive** | +4.9 | +4.8 | 0.7 | 13 (10) | 0.0, +0.1 |
| `s52-cal-4` | b) retry, 90 GiB hog at minute 12.0 | PASS, **inconclusive** | +6.5 | +6.4 | 0.7 | 12 (9) | -0.1, -0.4 |
| `s52-cal-3` | c) leak build, 3 MiB per change | **FAIL** `Anonymous growth 32.8 > 10.0` | +32.8 (130.5 → 163.3) | +32.7 | 0.9 | 13 (10) | +0.3, +0.1 |

- **a) passes** on its pre-registered conditions. Ambient file steal over R3 was 15.9 GiB.
- **c) passes**: R3 FAIL with `Anonymous growth` in the reasons and `changesAfter5` >= 4, so the leak control holds and (a) is not vacuous.
- **b) is not shown, twice.** The hog ran as registered and the did-it-run checks hold: it began at soak minute 12.0, `pgsteal_file` advanced 41.7 GiB (45 GiB read in 22 s) in `s52-cal-2` and 88.8 GiB (90 GiB in 59 s) in `s52-cal-4` between the minute-12 and minute-15 samples, swap was full, and R3 PASSed with no VOID. But the app's RSS did not fall (`reclaimMiB` 0.7 both times: 219.0 → 218.9 and 219.5 → 220.2 across the hog), so the pre-registered condition `reclaimMiB >= 30` was not met. The registered retry is used up. What this does show: 133 GiB of file reclaim lost the app nothing, as in the DIAG finding that volume does not decide whether the app's pages go. What it does not show is the gate's behaviour when RSS really falls; that rests on the ANON matrix in `test_gate.py` (anon flat, RSS down 65 MiB, USS and PSS +20: PASS) and not on a live run.
- **Reference drift** stayed within ±0.7 MiB in all four runs, so the drift exemption changed no verdict here.
- **Margin:** quiet-host anon growth was +4.9 to +6.5 MiB against the 10 MiB bar over the three non-leak runs.

### Amendment 20, revision 2 (2026-10-23: swap counts as growth; approved by the coordinator for Nate; written before `s52-cal-5`)
**Why.** The host's swap was freed, so anonymous pages can now be swapped out. smaps_rollup `Anonymous` excludes swapped pages, so a swap-out could shrink it and hide a leak into a false PASS.

**Rule (replaces "judged on `anon`" above).** R3 growth is the median of minute 5 against minute 30 of `anon + swap`: smaps_rollup `Anonymous` plus `Swap`, summed over the whole process tree, same 10 MiB bar. The FAIL reason is `Anonymous+Swap growth <x> > 10.0` (the leak control needs that text); the result key stays `anonGrowth`. The sampler records `swap` in every memory sample.
- **Fail closed.** A sample without a numeric `anon` or `swap` makes R3 VOID with `R3 samples lack anon or swap, which Amendment 20 judges R3 growth on`, the same as a missing `anon`.
- **Matrix rows:** `Anonymous` falling 30 MiB while `Swap` rises 30 leaves growth unchanged; a leak hidden by a swap-out (anon -20, swap +35) is still FAIL 15.0; swap growth alone over the bar FAILs; a missing or null `swap` is VOID.

**The calibration above ran on the anon-only gate.** Its samples carry no `swap`, so it was not re-run under revision 2, and the replay of `s52-cal-1` to `-4` under the new gate would be VOID. The leak FAIL (`s52-cal-3`, anon +32.8) holds under revision 2 by construction as long as swap does not fall: swap is non-negative and adds to anon, so a leak that raises anon cannot lower the sum unless pages swapped out before minute 5 are freed or swapped back in later; that was not measured and is not re-run.

**One quiet calibration, `s52-cal-5`, pass conditions fixed before the run.** Same binaries, quiet host (`--host-calm` first), flock held, no hog, same runner. Pass: `swap` numeric in all 180 samples; R3 PASS; `anonGrowth` (anon + swap) <= 10 and reported with its anon and swap parts; `len(changes)` >= 8; both reference drifts and `reclaimMiB` reported. A VOID or FAIL for any other reason is inconclusive and reported as such; it is not repeated without a new instruction.

**Calibration (b), as accepted.** The live eviction run is inconclusive: on the now-calmer host a 133 GiB file-reclaim burst did not move the app's RSS, so live eviction was not reproduced. The property (file-only reclaim leaves `Anonymous` untouched and does not VOID R3) rests on the S52 stand-in experiment (`pgsteal_anon` 0, anon unchanged) and on the unit matrix, not on a live gate run.
