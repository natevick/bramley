# Spike 4: JS engine for unmodified musickit.js (pre-registered 2026-10-09)

**Question.** Which embeddable engine can evaluate Apple's musickit.js v3 unmodified, with minimal host objects, within the 40 MiB slice of the budget?

**Candidates.**
- QuickJS-ng through `rquickjs`.
- V8 through `deno_core`, optionally with deno_web, deno_fetch and deno_webstorage.
- Boa only if both of those fail.

**Steps.**
1. Load the unmodified musickit.js bytes. The spike may read the analysis copy in `spikes/02-musickit-surface/vendor-cache/`; the product fetches it from Apple's CDN.
2. Use stub host objects plus a logging proxy that records every global and property musickit.js touches.
3. Call `MusicKit.configure({developerToken: "<dummy>", app: {name, build}})`.
4. Run until configure resolves, rejects, or issues its first network request.

**Pass, per engine.** Both conditions must hold:
- The `MusicKit` global is defined and `configure` is reached.
- The engine process is ≤ 40 MiB PSS median over 30 s idle after that point.

Also record:
- the time from process start to the `MusicKit` global being defined;
- the binary size;
- the full list of host APIs touched (the API contract seed).

**Did-it-run.**
- Log `typeof MusicKit`, the MusicKit version string, and the URL of the first fetch the engine attempted, or the exact error that stopped it.
- A sabotage run that truncates the script bytes **in memory only** must fail to define `MusicKit`. This proves the check can fail.

**Named false positives.**
- Stubs that return success for everything can make configure "work" while nothing is real. Report which stubs were hit, and say whether configure reached a real network request.
- An engine process smaller because work moved into a child process: walk the tree.
