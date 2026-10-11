# S4 CI developer token: pre-registration (2026-10-10)

Fixed before any run. S4a is the workflow that mints and publishes the token; S4b is the app fetching it.

## S4a (workflow)
1. **Throwaway-key mint verifies:** `node --test scripts/mint-developer-token.test.mjs` passes. It mints with a fresh in-memory P-256 key and checks `alg`/`kid`/`iss`, `exp = iat + 180 days`, no `origin` claim, and that the signature verifies against the public key. A token with one flipped signature byte must fail verification in the same test; otherwise the verifier is VOID.
2. **actionlint** prints nothing and exits 0 on `.github/workflows/developer-token.yml`.
3. **Static review:** the triggers are `schedule` and `workflow_dispatch` only. Permissions are exactly `contents: read`, `pages: write`, `id-token: write`. The job is guarded to the default branch. Every `uses:` is pinned to a 40-hex commit. Secrets appear only in the mint step's `env:`, and no step prints the token or the key.
4. **After merge (Nate dispatches):** a plain dispatch is green, and the published file's `exp` is at least 150 days out. A dispatch with `sabotage: true` signs under a wrong key id, so Apple's catalog check must answer non-200 and the run must fail without deploying (the published `iat` stays the same).

## S4b (app)
Every test below runs against a local fixture server and must fail on a mutation of the code it covers, then pass on the restored code. Each mutation is logged in RESULTS.
- 100 days cached → 0 requests.
- 20 days cached → 1 fetch; the cache is replaced, mode 0600.
- 20 days cached plus HTTP 500 → the cached token is used, with a warning.
- No cache plus a 500 → the config-dir file; with no config-dir file either → an error that names the developer token.
- Malformed, expired, oversize (64 KiB + 1) or a redirect to another host → the cache is untouched.
- 10 days plus a failed fetch → a warning, and `devTokenDaysLeft` = 10 in status.
- A 401 from Apple → exactly one refetch. If the refetched token is new, the call is retried and succeeds; if it is the same one, the existing developer-token-rejected banner shows (never "sign in").

## Gate
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
- `sg render -c 'bash scripts/acceptance.sh s4'` on a quiet box gives a PASS verdict. Pages serves 404 until the first dispatch, so this run proves the config-dir fallback keeps the harness working.
- `scripts/check-shipped-features.sh` on the release `bramley` exits 0.
- **Leak grep** before each commit: the staged diff contains no fragment of the real developer token, no `eyJ`, and no `PRIVATE KEY`.
