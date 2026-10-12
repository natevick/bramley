# S14a: Home recovery and shipped navigation: pre-registration (2026-10-12)

Written before any S14a probe or run. It amends the S13 harness only for F7 and F10, the parts S14a owns. The S13 fixer owns the rest of `drive.py` and `gate.py`, and S13's own Amendment 4.

## App changes under test
- **Home retries a failed chart.**
  - Steps: 5 s, 30 s, then every 60 s.
  - Each wait is the longer of the step and Retry-After.
  - It retries only while Home is shown.
  - It stops on sign-out, a 403, or a rejected developer token.
- **Shipped navigation.** `bramley open <view>` and `bramley scroll top|end`, plus the MCP tools `open` and `scroll`.

## F7: on the shipped binary (withdraws S13 deviation 1, which is no longer needed)
- **Driver:** in the shipped `browse` session it runs:
  1. MCP `library_playlists`;
  2. `open playlist:<largest id>`;
  3. `scroll end`.
  It then waits up to 120 s.
- **PASS** needs all of these:
  - MCP lists ≥ 1 playlist;
  - the largest playlist has ≥ 100 tracks;
  - both CLI exits are 0;
  - the view's `tracks.total` equals the API's count for the largest playlist;
  - `loaded == total == shownTo`, with more than one page;
  - `artTextures > 0`.
- The dev build only takes screenshots now; it writes no F7 evidence.
- **F9:** expects 12 tools, the existing 10 plus `open` and `scroll`.

## F10
- **(a) Bad id.**
  - The banner is read before `play 1`.
  - PASS: the app is alive (same pid), and either the command is refused, or a banner appears that differs from the one before. This means a banner left over from F9 can't satisfy the check.
- **(b) Cold start offline, in one session, under `bwrap --unshare-net`.**
  1. The app starts with no network.
  2. Search errors in both the CLI and the view.
  3. With the network still down, the driver runs `open home`.
  4. It attaches slirp4netns. That is the last input.
  - PASS needs all of these:
    - covers reach `COVERS` within 90 s;
    - the next search gives results;
    - the app is alive.
- **(c) Mid-session drop (review M7), in the same session after (b).** By then pooled API connections exist.
  1. The driver kills slirp.
  2. Search must error in the CLI and the view.
  3. It re-attaches slirp.
  4. The first search afterwards must give results: CLI exit 0, and the view shows results within 10 s.
  - The app must be alive with the same pid at the end. `aliveAtEnd` is now gated.
- **Did it run:** the driver makes a TCP connect probe to the API's IPv4 address from inside the app's network namespace (`nsenter -U -n`). It runs four times: cold, after attaching, after the drop, and after re-attaching. They must read **down, up, down, up**. Any other pattern FAILs, because then the network state the check claims was never produced.
- **Known limit:** Home builds a fresh HTTP agent on every chart attempt, so its retry never reuses a pooled connection. Leg (c) therefore tests pooled-connection recovery through search (the API worker's long-lived agent), not through Home. Home is not refetched in (c), because its covers are already loaded.

## Expected, stated before data
- F7 PASS; F10(a) PASS; F10(b) PASS, with covers back about 23 s after attaching (the 35 s retry slot).
- **F10(c) is the open question.** If the API worker reuses a dead pooled connection without retrying, the first search after re-attaching fails. That is an app finding: it gets reported and fixed in the API client, not waved through.
- Unchanged from S13 and owned elsewhere: F5 and R5 are expected to FAIL (engine causes, S14b).

## Controls
- **Old binary.** The same F7 and F10 driver runs against a release build of b059db7, before this work. Expected:
  - F7 FAIL: `open` is not a command, so the CLI exits 2;
  - F10(b) FAIL: covers never return.
  - This shows the driver can fail on exactly the bugs fixed here.
- **Gate:** `test_gate.py` covers each new rule, and `mutants.py` covers its F7/F10 mutants.

## Run plan
- **Targeted probes:**
  - only F7 and F10 (no audio is played; F10(a) is not in the probe);
  - in a `cs.session` with the unique prefix `bramley-s14a`, so the null sink is unique too;
  - only after the pre-flight is clear, and never while another gate runs.
- **The full `acceptance.sh s14a-1`** runs only when the coordinator says so: the S13 fixer is adding a flock and per-run sinks first. A VOID run is re-run and reported.
