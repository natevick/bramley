# Spike 4 results: JS engine for unmodified musickit.js (2026-10-09)

**Pre-registration:** `docs/spike-4-js-engine-PREREG.md`. The bar was 40 MiB PSS median over 30 s of idle after the point, with `MusicKit` defined and `configure` reached.

**musickit.js under test:** sha256 `21908b72bdcdeea3ec91fa40b105d0b9a2ba07c2a3fe32ea77edcd26e735012c`, ETag `a62a1503…`, `MusicKit.version` `3.2526.0-prerelease.x`. It was evaluated byte for byte as its own classic script.

## Verdicts
- **QuickJS-NG 0.16 (rquickjs 0.14): PASS** at 8.5 MiB.
- **V8 (deno_core 0.412, v8 150.4): PASS** at 33.2 MiB.

| Run | PSS med / max | RSS med | USS med | Threads | `MusicKit` defined after | musickit eval | Binary (stripped) |
|---|---|---|---|---|---|---|---|
| quickjs-plain | 8.5 / 8.5 | 10.9 | 8.4 | 1 | 53 ms | 51 ms | 1.7 MB |
| quickjs-logged | 8.4 / 8.4 | 10.8 | 8.4 | 1 | 56 ms | 53 ms | 1.7 MB |
| v8-plain | 33.2 / 34.0 | 35.9 | 33.2 | 5 | 50 ms | 40 ms | 50.1 MB |
| v8-logged | 32.7 / 33.5 | 35.3 | 32.7 | 5 | 54 ms | 43 ms | 50.1 MB |

MiB; 15 samples per run; always one process; a prior-build rerun reproduced 8.4 / 33.6. Times are from `main()` start. **plain:** ordinary globals (product shape). **logged:** a logging Proxy as the global's prototype.

## What ran
In every passing run:
- `typeof MusicKit === "object"`.
- `MusicKit.configure({developerToken: <dummy JWT>, app})` **resolved**.
- `isAuthorized` was false and `storefrontId` was "us".

**False-positive check:** `configure` resolved without any network request. Only a post-configure probe, `api.music("/v1/catalog/us/songs/1440833098")`, issued `GET https://api.music.apple.com/v1/catalog/us/songs/1440833098` (header: `authorization`). The stub rejected it, so nothing reached Apple; the fetch path is proven only to request construction.

## Sabotage proof
The script was truncated to 50%, in memory only.
- **QuickJS:** `Error: unexpected end of string` at musickit.js:28:178943.
- **V8:** `SyntaxError` at 28:178938.
- Both: `MusicKit` undefined, configure not reached, no samples, **VOID**. The check can fail.

## Answers to the two questions
1. **Format.** It is a UMD bundle with no ESM syntax and no `import()`. With no `exports`, `module` or `define` present, it assigns `(globalThis||self).MusicKit` directly. It evaluates unmodified as a classic script in both engines.
2. **What `MusicKit` needs before it is defined.**
   - Nothing asynchronous: the global is set synchronously during evaluation.
   - No DOM readiness and no `document.currentScript` (zero references).
   - **It does hard-require `Document` and `HTMLElement` constructors.** They are dereferenced at module scope by the fullscreen helpers, so V8 threw `ReferenceError: Document is not defined` until stubs existed.
   - After definition, it reads `<meta name="apple-music-developer-token">` through `document.head.querySelector`. It checks `document.readyState` and only waits for `DOMContentLoaded` if meta auto-configure applies.
   - `musickitloaded` is an **output**: MusicKit fires it through `setTimeout(() => document.dispatchEvent(new Event(...)))`. It is not a precondition.
   - Web components load only if the musickit `<script>` tag carries `data-web-components`.

## Caveats
- **QuickJS bends the spec in logged mode.** With a Proxy in the global prototype chain, unresolvable bare identifiers yield `undefined`, not `ReferenceError` (`refCheck: no-throw`), which hides hard requirements. Plain mode throws and still passes, so the result is not an artifact. V8 logged runs are the spec-correct contract recorder.
- **Idle only.** hls.js loads lazily at player init (static reading) and was not loaded; no EME, MSE, media, buffers or network ran. Playback memory and **QuickJS interpreter CPU** for hls.js are unmeasured. V8's ~7 MiB idle headroom tightens once hls.js loads.
- **Storage keys.** MusicKit read `localStorage["music.<iss lowercased>.media-user-token"]` (here `music.spikedummyteam.media-user-token`) and then removed it. This is a lead for spike 2's token-injection question. Only key names were logged, never values.

## Recommendation
**Use QuickJS-NG through rquickjs.**
- It takes about a quarter of V8's memory: 8.5 vs 33 MiB, which frees about 30 MiB for the CDM and UI slices.
- The binary is about 1/30th the size: 1.7 MB vs 50 MB.
- Startup is the same: about 55 ms to `MusicKit`.

Keep V8 as the fallback, and as the spec-correct contract recorder, until a playback-time spike measures QuickJS CPU and memory with hls.js and real segments.

## Re-run
```
cd /home/norm/projects/Personal/bramley/spikes/04-js-engine
(cd quickjs && cargo build --release) && (cd v8 && cargo build --release)
python3 -I run.py quickjs plain quickjs-plain && python3 -I run.py quickjs logged quickjs-logged
python3 -I run.py v8 plain v8-plain && python3 -I run.py v8 logged v8-logged
python3 -I run.py quickjs plain sabotage-quickjs --truncate 0.5 && python3 -I run.py v8 plain sabotage-v8 --truncate 0.5
```
Raw JSON is in `spikes/04-js-engine/results/`.
