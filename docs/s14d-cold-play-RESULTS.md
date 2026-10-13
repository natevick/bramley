# S14d cold-launch play that never starts: results (2026-10-13)

Pre-registration: `docs/s14d-cold-play-PREREG.md`. Base: main 3106814. Fix: 6ee504e.

## Verdict
- **Root cause: Apple's license server, not our platform.**
  - **Evidence:** in every failure whose license reply was logged (3 of 5), Apple answered `HTTP 200` with a 678-byte JSON error: `status: -1002`, `failureType: "3062"`, a customer `dialog`, and `errorCode: 0`.
  - **Passing launches** received a 1069-byte license.
  - MusicKit raised no `mediaPlaybackError`, so the app sat at Playing, position 0, with no banner.
- **Fixed in our layer: the hang is no longer silent.** A start that never advances now raises `PlayerError::Playback` within 10 s (6ee504e).
- **The pre-registered bar (0 failures in ≥ 25 cold launches) has not been run.** It assumes a cause we can fix; the evidence points to Apple's reply, which no platform change alters. Amendment proposed below for the coordinator.

## What the failure looks like
probe-lite: 10 launches, shipped-like build with net and CDM-event logging only.

| | Passing launch (cold-7) | Failing launch (cold-8) |
|---|---|---|
| webPlayback POST | 200, 8809 B | 200, 8809 B |
| CDM init, cert, `generateRequest` | yes | yes |
| newSession and message events | yes | yes |
| License POST to `play.itunes.apple.com` | 200, **1069 B** | 200, **678 B** |
| `update()`, second CDM FileIO, keys event, audio stream | yes | **none** |

- **What the 678 bytes are** (probe-shape; JSON shape only, no payload): `{"status":-1002,"failureType":"3062","errorCode":0,"customerMessage":<34 chars>,"dialog":{…},"cancel-purchase-batch":true,…}`.
- **What MusicKit does with it:**
  - MusicKit's `processJsonError` maps neither `3062` nor `-1002`, and `errorCode: 0` doesn't override `status`.
  - It then dispatches `playbackLicenseError` on its key session.
  - That error never reached `mediaPlaybackError`. probe-js logged no `mediaPlaybackError`, and play's promise resolved at +642 ms with `playbackError` still null.
  - This is MusicKit-internal, read only and never patched.
- **v1-1 f4cold-2 matches:** its log stops at the same point (the first FileIO after `generateRequest`) and it ended with the same status.
- **Not the cause:**
  - no CDM callback on a foreign thread (probe-thread);
  - no host-object exception, no unhandled rejection besides MusicKit's own keySystem probes, and no console error (probe-combined).
- **What 3062 means is unknown.**
  - 3 more launches meant to capture Apple's `customerMessage` all passed. I stopped there, at the ~60-play cap.
  - The failures clustered in time (09:46–09:53, then passes from 09:55), with no co-tenant on this box. That fits account-level state at Apple: throttling, or the same account playing elsewhere. Concurrent playback on the account from another device or agent is a live candidate.

## Loop table (fresh process per launch, own weston and null sink, GPU lock held, ≥ 10 s apart)

| Build | Time | Launches | PASS | FAIL | VOID |
|---|---|---|---|---|---|
| trace (3106814 + trace) | 09:38–09:42 | 20 | 20 | 0 | 0 |
| probe-thread | 09:43 | 2 | 2 | 0 | 0 |
| probe-lite | 09:45–09:47 | 10 | 8 | 2 | 0 |
| probe-shape | 09:50 | 1 | 0 | 1 | 0 |
| probe-js | 09:53 | 1 | 0 | 1 | 0 |
| probe-msg | 09:55 | 1 | 1 | 0 | 0 |
| probe-combined (60 s apart) | ~10:00 | 3 | 3 | 0 | 0 |
| **Total on 3106814 builds** | | **38** | **34** | **4** | **0** |

- **Plays used:** 38 of the ~60 cap. None were on the fix commit.
- **Deviation from the method:** the failures were captured on probe builds (small net/CDM logging), not the trace build. The trace build ran first and saw none.
- Per the PREREG, only 4xx/5xx is VOID. These were HTTP 200, so they stay FAILs.

## Fix (6ee504e): no silent hang
- **Trigger:** after setQueue, play, next, prev or jumpTo, if MusicKit reports Playing or Loading and the position has not advanced within 10 s, the snapshot carries `PlayerError::Playback("playback did not start within 10 s; the license or the network may have failed")`, and the app shows it as a banner.
- **Clears** as soon as the position advances.
- **Never fires** when paused, after a pause, or after playback has advanced once. A seek re-arms it only while a start is pending.
- **Public state only;** nothing is retried, so a possibly throttled account sees no extra license traffic.
- **Tests:**
  - a frozen start raises the error inside the bound and clears when time moves;
  - advancing playback, a later seek and a pause don't raise it.
  - The mutations "watchdog off", "never disarm" and "seek always arms" are each killed.

## Proposed Amendment 1, for the coordinator to approve or reject
- **Outcomes:** count cold launches in three classes:
  - **silent stall:** no banner within 10 s and no audio. FAIL, our fault.
  - **Apple license denial surfaced:** a banner within the bound. Reported separately, not a FAIL.
  - **PASS.**
- **New bar on 6ee504e:** 0 silent stalls in ≥ 25 cold launches. Apple denials are counted and reported.
- **Not added, and offered as options:**
  - an automatic retry of the license step. That is a product and rule-8 call, and it would add traffic to a possibly throttled account.
  - surfacing Apple's own `customerMessage`. That needs MusicKit to expose it, which it doesn't here.

## Amendment 1 results: the fast-relaunch hypothesis is falsified, and Apple's message was not captured

**Per-arm table** (10 pairs, 20 plays, alternating A/B, ≥ 10 s between pairs, 10:40–10:52, no co-tenants). The full table is in `docs/results/s14d-relaunch-pairs.json`.

| Arm | Gap kill → next play | Pairs | First launch PASS | Second launch PASS | Refusals |
|---|---|---|---|---|---|
| A (3 s) | 4.2–4.4 s | 5 | 5 | 5 | 0 |
| B (90 s) | 91.3–91.8 s | 5 | 5 | 5 | 0 |

- **Hypothesis falsified, per the pre-registered rule:** Arm A had 0 refusals, the same as B. A relaunch ~4 s after killing a playing process is not refused.
- **Apple's `customerMessage`: not captured.** All 20 license POSTs returned a license. The probe logged only the time, plus failureType/status/errorCode/customerMessage on refusal. No early stop.

**Timing pattern across all 58 plays today.** Descriptive only; no threshold is claimed.

| Window | Plays in the previous 10 min at each launch | Outcome |
|---|---|---|
| 09:38:41–09:46:13 | 0 → 29, launches ~10 s apart | 30 PASS |
| 09:46:23–09:53:13 | 30, 31, 23, 13 | **4 FAIL**: every refusal of the day |
| 09:55:24–10:01:19 | 9 → 3 | 4 PASS |
| 10:40:28–10:51:48 (pairs) | 0 → 18 | 20 PASS |

- **The refusals began at the 31st play in ~8 minutes**, then persisted for ~7 minutes even with launches 64–182 s apart, and stopped once the recent volume fell.
- **This fits an account-level rate limit with a cool-down** better than any per-launch race.
  - The v1-1 F4 failure also came after many plays in that acceptance run.
  - It is a pattern from one episode, not a measured threshold.
- **Plays used:** 38 earlier plus 20 now = 58 of the ~60 cap.

**Implications**
- Nothing in our platform causes the refusal. The watchdog (6ee504e) makes it visible within 10 s.
- **Harness note for the coordinator:** acceptance plus loops on one account can exceed whatever volume Apple tolerates. Spacing or capping plays per 10 min would keep F4 from measuring Apple's throttle.
