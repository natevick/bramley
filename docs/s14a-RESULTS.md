# S14a: Home recovery and shipped navigation: results (2026-10-12)

The pre-registration is `docs/s14a-PREREG.md`, with Amendment 1. The full `acceptance.sh` has **not** run: the coordinator is holding it. Every probe below ran under `/run/user/1000/bramley-gpu-session.lock`, with a unique null sink and the routing guard, and played no audio.

## Probes (`target/s14a-probe/`)

| Probe | Binary | F7 | F10 |
|---|---|---|---|
| new-1 | branch, before the API fix | PASS | FAIL: (c) the first search after re-attach failed |
| new-2 | same | (diagnosis) | (c) `afterDrop`: `Connection reset by peer`, 6 ms |
| new-3 | branch, with the reset fix (da83aa0) | PASS: exits [0,0], view total 250 = largest 250, end reached, 26 art textures | PASS: probes down/up/down/up, covers back 27.5 s, `afterDrop` exit 0 in 205 ms |
| old-1 | b059db7 (control) | FAIL: `open`/`scroll` exit 2 | FAIL: covers never return, and (c) fails |
| review round | — | **blocked** | **blocked** |

**Why the review round is blocked.** The real user token now gets 403 on `/v1/me/storefront`; the catalog itself answers with the developer token alone. But every F10 network leg needs the storefront:
- Home's chart path checks sign-in through `/v1/me/storefront`;
- the app's search resolves its storefront through `/v1/me/storefront` before calling the catalog.

So none of F10(b), (c) or (d) can run on the developer token alone. They, and F7, wait for Nate to re-sign in.

## Review findings

| ID | Status | Evidence |
|---|---|---|
| B1 silent dead pooled socket | **fixed** | API (6e42391, 46c97f4) and art agent (bac76c8). Details below. |
| M1 Retry-After overflow | **fixed** | `retry_after` clamps to 3600 s; the value table includes `18446744073709551615 → 3600` |
| M2 pinned scroll spins | **fixed** | Repaint only while `Library::due(now)`; the pin is dropped when signed out. Tests: repaint delay > 1 s inside a failed page's backoff; `scroll()` is `None` after sign-out |
| L1 chart 401 retried | **fixed** | 401 becomes `DeveloperTokenRejected`; new `(200, 401)` row: no retry, 2 requests |
| L2 HTTP-date Retry-After | **fixed** | An unreadable value waits 60 s (`Wed, 21 Oct 2099 … → 60`). No httpdate dependency |
| L3 unbounded mutant hangs | **fixed** | Shared `keepalive` helper with `incoming().take(n)` |
| L4 fixed 300 ms sleep | **fixed** | `Grid::retry` returns whether it started; positive asserts poll to a deadline |
| L5 late open/scroll applied | **fixed** | A request past its deadline gets LATE before any arm (also covers SignIn/SignOut) |
| L6 F10(c) timing | **fixed** | `dropGapS` ≥ 15 s VOIDs F10; F10(d) black hole added. Pre-registered in Amendment 1 before running |
| L7 app retry wiring | **fixed** | App test: no retry after 6.5 s on Search; a retry within 1 s on Home |
| Info: `open` id length | **fixed** | Capped at 64 characters |
| Nit: shared backoff const | **fixed** | Home has its own `BACKOFF` |
| L8 partial cover failures not retried | **declined** | Out of scope: S14a recovers a failed *chart*. Holes left by single covers refill on zoom or sign-in; a per-cover retry is a separate change |
| L8 `open album:l.x` → 404 | **declined** | Harmless: the view shows an error and no extra data. A click can't reach it, and validating id shape per route would duplicate the API's own rules |
| Budget cut: `Refused` re-wrap | **declined** | Source is +122/−64 against a budget of about 250. The cut changes `load_chart`'s error type through `fetch` for about 8 net lines. `stale()` is gone, and the shared test helper is in |

**B1 detail.**
- **API:** a 10 s receive-response timeout. On any transport error the pool is dropped and the request retried once on a non-pooled agent.
  - Test: a socket that goes silent is given up in 10–13 s and retried fresh. Before the fix it failed at 15 s.
  - Test: the dead sibling is never used after the pool drop.
- **Art agent:** the same 10 s timeout, pool drop and fresh retry for library, search and Home art. Test: a silent art socket is retried fresh in under 13 s.

## Mutation: every new test fails without its fix
Each mutant ran under the GPU lock, in a separate worktree.

```
KILLED  api: no fresh retry            (pooled tests: 3 failed)
KILLED  api: no receive timeout        (silent test: 15.37 s)
KILLED  api: pool not dropped          (dead-sibling test)
KILLED  api: Retry-After unclamped / HTTP-date waits 1 s
KILLED  covers: no fresh retry / no receive timeout (silent art test)
KILLED  covers: chart 401 retried
KILLED  covers: retry() claims it started
KILLED  M2: repaint on `wanted`        KILLED  M2: pin kept on sign-out
KILLED  L5: late Nav applied           KILLED  L7: retry on every route
KILLED  id length uncapped
SURVIVED no-op control
gate: mutants.py 57 of 57 killed, no-op survived; test_gate.py 11/11
```

Two older tests changed expectation, because a transport error now gets its one retry:
- a storefront that hangs up without answering is now asked twice;
- the search-art test counts distinct paths.

## Checks
- `cargo test`: app 146, API 24, mcp_stdio 1.
- `clippy -D warnings`: clean on shipped, dev and gate-sabotage; `fmt --check` clean.
- **Leak grep:** library names were unavailable (403); token fragments only, 0 hits on every commit.
