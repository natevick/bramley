# S4 CI developer token: results (2026-10-10)

Pre-registration: `docs/s4-developer-token-PREREG.md`. Evidence lives in `target/s4/` and `target/acceptance/s4/` (both gitignored). Commits: S4a 59610c9, S4b 219d52d. The review round (`scratch/review-s4.md`) is in e624c1b, 3c018eb, 439329f and the compliance wording commit.

## S4a (workflow)

| # | Check | Result |
|---|---|---|
| 1 | Throwaway-key mint verifies | PASS: 2/2 `node:test`. Four mutations each turn it red: a 179-day lifetime, `verify` always true, an `origin` claim, and DER on both sides. The test now asserts a 64-byte raw signature; before that assertion, DER stayed green. |
| 2 | actionlint 1.7.12 | PASS, exit 0. A typo'd `inputs.sabotag` gives exit 1, so the linter can fail. |
| 3 | Static review | Triggers are `schedule` (Mondays 06:17 UTC) and `workflow_dispatch` only. Permissions are `contents: read`, `pages: write` and `id-token: write`. The job is guarded with `github.ref == 'refs/heads/main'`, a literal, because a schedule event's payload may not carry `repository.default_branch`. Secrets reach only the mint step's `env:`, as **environment secrets of `github-pages`**, which the job targets. Nothing prints the token or the key. After minting, `test -s site/developer-token.json` stops an empty site from deploying. |
| 3b | Live check can fail | PASS. A throwaway key gave `HTTP 401`, exit 1, and no file was written. The check is `/v1/catalog/us/search?term=a&types=songs&limit=1`, which answers 401 to junk tokens (5/5) and 200 to the real one (3/3). |
| 3c | Why not `/v1/storefronts/us` (review finding 7) | That endpoint, `/v1/storefronts` and `/v1/catalog/us/genres` answered 200 with no auth or a junk token, 5/5 each. A throwaway-key mint **passed** against storefronts/us, so it would make the live check a no-op. |
| 3d | Entry point through a symlink (review finding 3) | The old script exited 0 and wrote nothing. The new one resolves `realpath`, runs, and fails closed (401, exit 1). |
| 4 | Green dispatch with `exp` ≥150 days, and a red sabotage dispatch | **Pending: after merge, Nate dispatches.** Also confirm that the first scheduled run (Monday 06:17 UTC) executes the publish job instead of skipping it. |

**Pinned actions** (lightweight tags, resolved with `git ls-remote`):

| Action | Tag | Commit |
|---|---|---|
| actions/checkout | v7.0.1 | 3d3c42e5aac5ba805825da76410c181273ba90b1 |
| actions/upload-pages-artifact | v5.0.0 | fc324d3547104276b827a68afc52ff2a11cc49c9 |
| actions/deploy-pages | v5.0.1 | 368f82528645a54fb793d4d04e342629a3f51346 |

**Deviation:** the plan named `scripts/mint-developer-token.py`. The brief asked for Node's built-in crypto, so the script is `scripts/mint-developer-token.mjs`.

**Sabotage design:** `sabotage: true` signs under the key id `SABOTAGE00`. The signature still verifies against the public key, so only Apple's live catalog check can stop it. A red run therefore proves that check can fail, and nothing deploys.

### Runbook: scheduled runs stop after 60 days without repo activity
GitHub disables scheduled workflows in a public repo after 60 days with no activity. A 180-day token leaves at least 120 days after the last weekly run, and the app warns at 14 days. To re-enable:

```
gh workflow enable developer-token.yml -R natevick/bramley
gh workflow run developer-token.yml -R natevick/bramley
```

Then check the published file: `curl -s https://natevick.github.io/bramley/developer-token.json` shows `exp` 180 days out. This deploy owns the whole Pages site.

### Runbook: secrets and settings
- The three secrets (`MUSICKIT_P8_B64`, `MUSICKIT_KEY_ID`, `MUSICKIT_TEAM_ID`) are **environment secrets on `github-pages`**, with no repository-level copies.
- The environment's deployment branches are limited to `main` (Settings → Environments → github-pages).
- To rotate: replace them there; nothing in the workflow changes.
- **Never set a custom domain** on the `bramley` Pages site or on `natevick.github.io`. The app compiles in `https://natevick.github.io/bramley/developer-token.json` and follows no redirects, so a custom domain's 301 would fail every installed app's renewal.

## S4b (app)

| Check | Result |
|---|---|
| `cargo clippy --workspace --all-targets -D warnings`, plus `-p bramley --all-features` | PASS |
| `cargo test --workspace` | PASS: app 65 (baseline 55), api 15 (baseline 14), engine 98 |
| Mutations | 25/25 KILLED after the review round (list below) |
| Acceptance gate `sg render -c 'bash scripts/acceptance.sh s4'` on 219d52d, clean tree | **PASS.** R1 170.0 PSS / 143.2 USS / 202.6 RSS (zone 3, reference drift 3.6). R2 72.6. Ballast FAIL holds (USS 215.7, zone 1). `realStateUnchanged`. Status `devTokenSource: "config"`, since Pages served 404, which proves the fallback. |
| Gate re-run after the review round, on 666e40d (`s4r2` dirty, `s4r3` clean) | **VOID, twice; per acceptance.md that is not a PASS.** R1 171.3 PSS / 144.3 USS / 204.0 RSS (zone 3), R2 73.5. Ballast FAIL holds (USS 215.8). `realStateUnchanged` holds, including the real cache, absent before and after. `devTokenSource: "config"`. The VOID comes from reference drift of 5.2 and 5.3 MiB against the 5 MiB bar. The fixed reference binary (old main, none of this code) itself reads 88.5 PSS / 61.6 USS against 86.9 / 60.1 in the first S4 run. The app's margin over the same-session reference is unchanged: +83.1 then, +82.8 now. So the box drifted, not the app. Thresholds untouched; this needs a re-run on a fresher box, or Nate's call on re-recording the reference. |
| `check-shipped-features.sh` on the release `bramley` | exit 0 (`build.json` `shippedFeaturesExit: 0`) |
| Live smoke (`target/s4/live/smoke.py`): dev build with `--developer-token-url`, a local JSON fixture serving the real token, an isolated config holding **no** `developer-token` file, and an isolated `XDG_CACHE_HOME` | 50 real covers. 1 token request with UA `Bramley/0.1.0 (linux)`. Cache 0600. `devTokenDaysLeft` 150. |
| Live control: the same, with a corrupted token served | 0 covers. Exactly 2 token requests (the 401 refetch, once). The banner reads "Bramley's developer token was rejected", never sign-in. |
| Leaks | 0 token fragments and 0 `eyJ` in the gate evidence, the smoke logs, or either commit's diff |

**Mutations** (each was run with its named test, and every file was restored afterwards):
- **Cache freshness:** the fresh cache skipped; never renewing; the cache written non-0600.
- **Fallback:** no stale-cache fallback; no config-dir fallback; fetching on every call; no error recorded.
- **Warnings and status:** no warning on a failed renewal; no 14-day warning; no days in status.
- **Response validation:** `exp` unchecked; an empty token accepted; no 64 KiB cap; redirects followed.
- **401 refetch:** covers never refetch; the same token retried; the API never renews.
- **Wiring:** a missing token isn't a banner; the URL flag accepted outside dev.
- **Review round:**
  - **Retry:** latched forever after one failure; no first backoff; the cause lost while waiting; the cause not naming the host; a success keeping the error.
  - **Refetch:** unthrottled.
  - **Covers:** don't wait for the token.

**Review round, S4b behaviour.** While no usable token exists, a failed fetch is retried after 5 s, then 30 s, then every 60 s. The startup prefetch and covers wait on it, and the banner keeps the real cause ("couldn't reach natevick.github.io: HTTP 500") until a fetch lands, which clears it. A 401 refetch runs at most once per 60 s.

## Harness
- `isolated()` sets `XDG_CACHE_HOME` under the per-mode temp config.
- `assert_isolated` refuses an env with no cache, or one that resolves to the real cache.
- `realStateUnchanged` also covers the real `~/.cache/bramley`, metadata only.
- test_gate has 7/7 cases, including a changed cache and both refusals.

## Follow-ups
- **5, the iat-age alert:** an external freshness check on the published `iat`, alerting when it is more than 10 days old. The coordinator is setting it up separately.
- **9, engine refetch on 401:** MusicKit's configure gets whatever `get()` returns, and the engine does not refetch on its own 401.

## Open items
- **The acceptance "corrupted developer token" control:** once Pages is live, corrupting only the config-dir file no longer reaches the app. S13 should corrupt the served or cached copy instead, using a dev `--developer-token-url`.
- **Budget:** S4a 100 source / 32 tests against 120/60. S4b adds 301 source lines (266 net) and 280 test lines against 200/150, so it is +50% and +87% over. 188 of those source lines are the token module; the rest is wiring through four existing readers and the API client.
