# S14d cold-launch play that never starts: pre-registration (2026-10-13)

The first v1 run on main (3106814, `target/acceptance/v1-1` in the v1 worktree) FAILed F4 on 1 of 5 cold launches (f4cold-2). The app stayed at Playing with applied 2, position 0, sentMs 0, decryptedMs 0 and no banner. Its log stops after the CDM's first FileIO sequence. Passing launches go on to a second FileIO sequence that writes 149 bytes, which is the session/license step.

## Method
- **Repro loop:** each iteration launches a fresh app process in its own weston and null-sink session, with the routing guard. It holds `/run/user/$UID/bramley-gpu-session.lock` throughout.
  - The app uses an isolated config, the same as the harness.
  - The iteration waits for first paint and covers (or a banner), as `f4_cold` does, then issues one `play` of the F4 song.
- **Verdicts:**
  - **PASS:** position advances past 1 s within 15 s, which is F4's own window.
  - **FAIL:** anything else.
  - **VOID:** the launch never reached a signed-in, painted state (sign-in banner, crash, co-tenant), or Apple refused the token. VOID launches are reported and not counted.
- **Pacing:** launches start ≥ 10 s apart. The whole sprint stops at ~60 plays, at Apple's request.
- **Trace:** a non-shipped `trace` build (`--features trace`) logs EME calls, events and promise outcomes, CDM events and fetch completions. It records hosts and hashes only: no URLs, bodies, tokens or titles.
- **Diagnosis:** capture ≥ 2 FAILs on the trace build of 3106814, and diff each against a passing launch from the same loop.

## Pass bar (fix commit)
- **0 FAILs in ≥ 25 valid cold launches** of the shipped (non-trace) build of the fix commit.
- If the true failure rate were 20 % (1 in 5, as in v1-1), a no-op fix would show 0 in 25 with probability 0.8^25 ≈ 0.4 %. At 10 % it would be ≈ 7 %.

## Also reported
- **The failure rate on 3106814 from the repro loop:** shipped and trace launches are counted separately.
- **No silent hang:** if the license step cannot complete, a banner or error must surface within a bounded time. The bound is fixed in the fix commit and tested.

## Named false positives
- **Apple refusing the token (403)** reads as a never-starting play. A launch whose license or webPlayback POST returned 4xx/5xx counts as VOID, not FAIL, and is reported.
- **Co-tenant runs** are excluded by the lock plus the preflight.
- **A loop that never played anything:** the loop records `play`'s exit code and the applied sequence number for every launch.
