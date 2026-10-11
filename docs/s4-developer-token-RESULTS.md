# S4 CI developer token: results (2026-10-10)

Pre-registration: `docs/s4-developer-token-PREREG.md`. Evidence lives in `target/s4/` and `target/acceptance/s4/` (both gitignored). Commits: S4a 59610c9, S4b 219d52d.

## S4a (workflow)

| # | Check | Result |
|---|---|---|
| 1 | Throwaway-key mint verifies | PASS: 2/2 `node:test`. Three mutations each turn it red: a 179-day lifetime, `verify` always true, and an `origin` claim. |
| 2 | actionlint 1.7.12 | PASS, exit 0. A typo'd `inputs.sabotag` gives exit 1, so the linter can fail. |
| 3 | Static review | Triggers are `schedule` (Mondays 06:17 UTC) and `workflow_dispatch` only. Permissions are `contents: read`, `pages: write` and `id-token: write`. The job is guarded with `github.ref == 'refs/heads/main'`, a literal, because a schedule event's payload may not carry `repository.default_branch`. Secrets reach only the mint step's `env:`. Nothing prints the token or the key. |
| 3b | Live check can fail | PASS. A throwaway key minted against Apple's catalog gave `HTTP 401`, exit 1, and no file was written. |
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

## S4b (app)

| Check | Result |
|---|---|
| `cargo clippy --workspace --all-targets -D warnings`, plus `-p bramley --all-features` | PASS |
| `cargo test --workspace` | PASS: app 62 (baseline 55), api 15 (baseline 14), engine 98 |
| Mutations | 19/19 KILLED (list below) |
| Acceptance gate `sg render -c 'bash scripts/acceptance.sh s4'` on 219d52d, clean tree | **PASS.** R1 170.0 PSS / 143.2 USS / 202.6 RSS (zone 3, reference drift 3.6). R2 72.6. Ballast FAIL holds (USS 215.7, zone 1). `realStateUnchanged`. Status `devTokenSource: "config"`, since Pages served 404, which proves the fallback. |
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

## Open items
- **Harness cache isolation:** gate.py doesn't set `XDG_CACHE_HOME`. Once Pages is live, gate runs will write the real `~/.cache/bramley/developer-token.json`, which holds the public token and no secret. Proposed one-line fix for S13: add `XDG_CACHE_HOME` under the isolated config dir in `covers-session.py isolated()`.
- **The acceptance "corrupted developer token" control:** once Pages is live, corrupting only the config-dir file no longer reaches the app. S13 should corrupt the served or cached copy instead, using a dev `--developer-token-url`.
- **The engine doesn't refetch on its own 401:** MusicKit's configure gets whatever `get()` returns. In practice the covers check at startup renews first.
- **Budget:** S4a 100 source / 32 tests against 120/60. S4b adds 301 source lines (266 net) and 280 test lines against 200/150, so it is +50% and +87% over. 188 of those source lines are the token module; the rest is wiring through four existing readers and the API client.
