import json, re, sys, os
root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
d = json.load(open(os.path.join(os.path.dirname(__file__), 'results', sys.argv[1] + '.json')))
seed = open(os.path.join(root, 'docs', 'musickit-api-contract-seed.md')).read()
seed_names = set(m.split('.')[-1].rstrip('()=') for m in re.findall(r'`(global\.[A-Za-z0-9_.$]+)`', seed))
AREAS = [
    ('EME', r'^(MediaKey|Navigator\.requestMediaKeySystemAccess)'),
    ('MSE', r'^(MediaSource|ManagedMediaSource|SourceBuffer|TimeRanges)|^window\.(MediaSource|ManagedMediaSource|SourceBuffer)'),
    ('Media', r'^(HTMLMediaElement|HTMLAudioElement|HTMLVideoElement|TextTrack|AudioTrack|MediaSession|MediaMetadata|MediaCapabilities)|^Navigator\.(mediaSession|mediaCapabilities)|^window\.(Audio|MediaMetadata|HTMLMediaElement|HTMLAudioElement|HTMLVideoElement)'),
    ('Network', r'^(Response|Request|Headers|XMLHttpRequest|URL|URLSearchParams|Blob|AbortController|AbortSignal|NetworkInformation)|^window\.(fetch|Response|Request|Headers|XMLHttpRequest|URL|URLSearchParams|Blob|AbortController)|^Navigator\.(onLine|connection)'),
    ('Storage', r'^Storage|^window\.(localStorage|sessionStorage|indexedDB)|^Document\.cookie'),
    ('Events', r'^(EventTarget|Event|CustomEvent)|^window\.(Event|CustomEvent|EventTarget|addEventListener|removeEventListener|dispatchEvent)'),
    ('Timers', r'^Performance|^window\.(setTimeout|clearTimeout|setInterval|clearInterval|requestAnimationFrame|cancelAnimationFrame|queueMicrotask|performance)'),
    ('DOM', r'^(Document|Node|Element|HTML|DOMParser|MutationObserver|CSSStyleDeclaration|DOMTokenList|Location|History|Screen)|^window\.(document|Document|Node|Element|HTML|location|history|getComputedStyle|matchMedia|screen|innerWidth|innerHeight|devicePixelRatio)'),
]
def area(k):
    for a, rx in AREAS:
        if re.search(rx, k): return a
    return 'Misc'
rows = {}
for k, n, mk, hls, other in d['detail']['rec']:
    if mk + hls == 0: continue
    rows.setdefault(area(k), []).append((k, n, mk, hls, k.split('.')[-1].rstrip('()=') in seed_names))
total = sum(len(v) for v in rows.values())
seeded = sum(1 for v in rows.values() for r in v if r[4])
s = d['summary']
out = [f"# MusicKit JS host API contract, observed in Chrome (spike 2, {d['startedAt'][:10]})", "",
       "This lists every instrumented web API member that Apple's code called during a real, authorized, full-length playback session in headless Chrome 154.",
       f"- **Session:** load, configure, play Bohemian Rhapsody (flavor `{(s.get('item') or {}).get('flavor')}`), pause, seek to 120 s, resume, then skip to the next song.",
       f"- **Coverage:** {total} members; {seeded} also appear in the spike-4 seed (✓ means the member name is in `docs/musickit-api-contract-seed.md`).",
       "- **Method:** logging getters and method wrappers installed through `Page.addScriptToEvaluateOnNewDocument` on the prototypes listed in `spikes/02-musickit-surface/harness/instrument.js`, plus window globals. Spike-only, never shipped.",
       "- **Attribution:** an access is attributed to Apple's code when the stack contains `musickit/v3` or hls.js.",
       "- **Limits:** this is NOT the complete contract.",
       "  - `in`-operator feature probes are invisible.",
       "  - Non-configurable members such as `window.document` and `window.location` cannot be wrapped; their members on the prototypes are.",
       "  - Workers are not instrumented.",
       "  - The spike-4 seed remains the record of configure-time probes.",
       "", "| Area | Member | Calls (sampled) | musickit | hls.js | In seed |", "|---|---|---|---|---|---|"]
for a in ['EME', 'MSE', 'Media', 'Network', 'Storage', 'Events', 'Timers', 'DOM', 'Misc']:
    for k, n, mk, hls, ins in sorted(rows.get(a, [])):
        out.append(f"| {a} | `{k}` | {n} | {mk} | {hls} | {'✓' if ins else ''} |")
out += ["", "## Observed playback facts that the runtime must honour", ""]
open(os.path.join(root, 'docs', 'musickit-api-contract-chrome.md'), 'w').write('\n'.join(out) + '\n')
print(total, seeded)
