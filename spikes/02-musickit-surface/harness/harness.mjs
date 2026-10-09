import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const CHROME = '/opt/google/chrome/chrome';
const NULL_SINK = 'spike2-null';
const RUNTIME_DIR = '/run/user/' + process.getuid();
const CONF = path.join(os.homedir(), '.config', 'bramley');
const arg = (k, d) => { const i = process.argv.indexOf('--' + k); return i > 0 ? process.argv[i + 1] : d; };
const flag = k => process.argv.includes('--' + k);
const name = arg('name');
const port = Number(arg('port', 8766));
const variant = arg('variant', 'real');
const instrument = !flag('no-instrument');
const contract = !flag('no-contract');
const outDir = path.join(HERE, 'results');
fs.mkdirSync(outDir, { recursive: true });
const sleep = ms => new Promise(r => setTimeout(r, ms));
const MiB = kb => Math.round((kb / 1024) * 10) / 10;
const median = a => { const s = [...a].sort((x, y) => x - y); const m = Math.floor(s.length / 2); return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };

const dev = fs.readFileSync(path.join(CONF, 'developer-token'), 'utf8').trim();
const mut = fs.readFileSync(path.join(CONF, 'music-user-token'), 'utf8').trim();
const teamId = JSON.parse(Buffer.from(dev.split('.')[1], 'base64url').toString()).iss;
const storageKey = `music.${teamId}`.toLowerCase() + '.media-user-token';
const corrupt = t => { const p = t.split('.'); const s = p[2]; p[2] = s.slice(0, 10) + (s[10] === 'A' ? 'B' : 'A') + s.slice(11, 20) + (s[20] === 'Q' ? 'R' : 'Q') + s.slice(21); return p.join('.'); };
const devForRun = variant === 'bad-dev' ? corrupt(dev) : dev;
const seedUser = variant !== 'no-user';
const redact = s => { let r = s.split(dev).join('<dev-token>').split(devForRun).join('<dev-token>').split(mut).join('<user-token>'); return r.replace(/eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g, '<jwt>'); };

function procs() {
  const m = new Map();
  for (const d of fs.readdirSync('/proc')) { if (!/^\d+$/.test(d)) continue; try { const st = fs.readFileSync(`/proc/${d}/stat`, 'utf8'); m.set(Number(d), Number(st.slice(st.lastIndexOf(')') + 2).split(' ')[1])); } catch {} }
  return m;
}
function tree(root) {
  const p = procs(); const kids = new Map();
  for (const [pid, pp] of p) { if (!kids.has(pp)) kids.set(pp, []); kids.get(pp).push(pid); }
  const out = []; const st = [root];
  while (st.length) { const x = st.pop(); if (!p.has(x) || out.includes(x)) continue; out.push(x); for (const c of kids.get(x) || []) st.push(c); }
  return out;
}
function rollup(pid) {
  try { const r = { Rss: 0, Pss: 0, Private_Clean: 0, Private_Dirty: 0 }; for (const l of fs.readFileSync(`/proc/${pid}/smaps_rollup`, 'utf8').split('\n')) { const m = l.match(/^(\w+):\s+(\d+) kB/); if (m && m[1] in r) r[m[1]] = Number(m[2]); } return r; } catch { return null; }
}
function mem(root) {
  const pids = tree(root); let pss = 0, rss = 0, uss = 0, n = 0;
  for (const p of pids) { const r = rollup(p); if (!r) continue; n++; pss += r.Pss; rss += r.Rss; uss += r.Private_Clean + r.Private_Dirty; }
  return { pids, count: n, pssKb: pss, rssKb: rss, ussKb: uss };
}
function pwRouting(pids) {
  try {
    const dump = JSON.parse(execFileSync('pw-dump', { env: { ...process.env, XDG_RUNTIME_DIR: RUNTIME_DIR }, maxBuffer: 64 << 20 }).toString());
    const nodes = new Map();
    for (const o of dump) if (o.type === 'PipeWire:Interface:Node') nodes.set(o.id, o.info?.props || {});
    const ours = [...nodes].filter(([, p]) => pids.includes(Number(p['application.process.id'])));
    const targets = [];
    for (const o of dump) { if (o.type !== 'PipeWire:Interface:Link') continue; if (ours.some(([id]) => id === o.info?.['output-node-id'])) targets.push(nodes.get(o.info['input-node-id'])?.['node.name']); }
    return { streams: ours.map(([id, p]) => ({ id, app: p['application.name'], pid: p['application.process.id'] })), targets: [...new Set(targets)] };
  } catch (e) { return { error: String(e).slice(0, 200) }; }
}
function defaultSink() { try { return (execFileSync('wpctl', ['inspect', '@DEFAULT_AUDIO_SINK@'], { env: { ...process.env, XDG_RUNTIME_DIR: RUNTIME_DIR } }).toString().match(/node\.name = "([^"]+)"/) || [])[1] || null; } catch { return null; } }

async function cdp(profile) {
  const pf = path.join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 200 && !fs.existsSync(pf); i++) await sleep(100);
  const p = fs.readFileSync(pf, 'utf8').split('\n')[0].trim();
  let page;
  for (let i = 0; i < 100 && !page; i++) { try { page = (await (await fetch(`http://127.0.0.1:${p}/json/list`)).json()).find(t => t.type === 'page'); } catch {} if (!page) await sleep(100); }
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  let id = 0; const pending = new Map(); const exceptions = [];
  ws.onmessage = ev => { const m = JSON.parse(ev.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } else if (m.method === 'Runtime.exceptionThrown') exceptions.push((m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text || '').slice(0, 300)); };
  const send = (method, params = {}) => new Promise(res => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
  const evaluate = async (expression, awaitPromise = false) => (await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise })).result?.result?.value;
  return { ws, send, evaluate, exceptions };
}

const STATE = `(() => { const r = window.__s2state ? window.__s2state() : { phase: 'no-page' }; const I = window.__s2i; if (I) { r.eme = { everUsable: I.eme.everUsable, firstUsableEpochMs: I.eme.firstUsableEpochMs, sessions: I.eme.sessions, statuses: Object.values(I.eme.statuses), messages: I.eme.messages.length }; const a = I.media; r.media = a ? { currentTime: a.currentTime, paused: a.paused, readyState: a.readyState, tag: a.tagName } : null; } return r; })()`;
const DETAIL = `(() => { const I = window.__s2i; if (!I) return null; const rec = []; for (const [k, v] of I.rec) rec.push([k, v.n, v.mk, v.hls, v.other]); return { rec, net: I.net, m3u8: I.m3u8, eme: I.eme, mse: I.mse, flavors: I.flavors || null, webPlaybackKeys: I.webPlaybackKeys || null }; })()`;

async function main() {
  const profile = path.join(HERE, 'profiles', `${name}-${Date.now()}`);
  fs.mkdirSync(profile, { recursive: true });
  const sinkBefore = defaultSink();
  const t0 = Date.now();
  const child = spawn(CHROME, ['--headless=new', '--no-first-run', '--no-default-browser-check', '--autoplay-policy=no-user-gesture-required', '--user-data-dir=' + profile, '--remote-debugging-port=0', 'about:blank'], { env: { ...process.env, XDG_RUNTIME_DIR: RUNTIME_DIR, PULSE_SINK: NULL_SINK }, stdio: 'ignore' });
  const root = child.pid; const seen = new Set([root]);
  const R = { name, variant, instrument, contract, startedAt: new Date(t0).toISOString(), samples: [], steps: [], notes: [] };
  let c;
  try {
    c = await cdp(profile);
    await c.send('Runtime.enable');
    await c.send('Page.enable');
    const origin = `http://127.0.0.1:${port}`;
    const seed = `(() => { if (location.origin !== ${JSON.stringify(origin)}) return; Object.defineProperty(window, '__BRAMLEY_DEV', { value: ${JSON.stringify(devForRun)}, enumerable: false }); ${seedUser ? `try { localStorage.setItem(${JSON.stringify(storageKey)}, ${JSON.stringify(mut)}); } catch (e) {}` : `try { localStorage.removeItem(${JSON.stringify(storageKey)}); } catch (e) {}`} })();`;
    await c.send('Page.addScriptToEvaluateOnNewDocument', { source: seed });
    if (instrument) await c.send('Page.addScriptToEvaluateOnNewDocument', { source: (contract ? '' : 'window.__S2_NO_CONTRACT = true;\n') + fs.readFileSync(path.join(HERE, 'instrument.js'), 'utf8') });
    await c.send('Page.navigate', { url: `${origin}/page.html` });
    let st; const until = Date.now() + 60000;
    while (Date.now() < until) { st = await c.evaluate(STATE); if (st && (/failed/.test(st.phase) || (st.currentPlaybackTime > 0.5))) break; await sleep(250); }
    R.firstAudioMs = st && st.currentPlaybackTime > 0.5 ? Date.now() - t0 : null;
    R.afterStart = st;
    if (!R.firstAudioMs) R.notes.push('no playback within 60 s; phase=' + (st && st.phase));
    const stop = Date.now() + (R.firstAudioMs ? 75000 : 12000);
    let midDone = false;
    while (Date.now() < stop) {
      const m = mem(root); m.pids.forEach(p => seen.add(p));
      const s = await c.evaluate(STATE);
      R.samples.push({ t: Date.now() - t0, count: m.count, pssKb: m.pssKb, rssKb: m.rssKb, ussKb: m.ussKb, cpt: s?.currentPlaybackTime, state: s?.playbackState, media: s?.media?.currentTime, usable: s?.eme?.everUsable });
      if (!midDone && R.samples.length >= 8) { R.routing = pwRouting(m.pids); midDone = true; }
      if (R.firstAudioMs && s?.currentPlaybackTime >= 45 && R.samples.length >= 15) break;
      await sleep(2000);
    }
    if (!R.routing) R.routing = pwRouting(mem(root).pids);
    R.beforeControls = await c.evaluate(STATE);
    if (R.firstAudioMs) {
      const step = async (label, expr, wait) => { let e = null; try { await c.evaluate(expr, true); } catch (x) { e = String(x); } await sleep(wait); const s = await c.evaluate(STATE); R.steps.push({ label, error: e, cpt: s?.currentPlaybackTime, state: s?.playbackState, item: s?.item?.id, media: s?.media?.currentTime }); };
      await step('pause', 'window.__s2pause()', 1000);
      await step('paused+4s', '0', 4000);
      await step('seek-120', 'window.__s2seek(120)', 2500);
      await step('play', 'window.__s2play()', 6000);
      await step('next', 'window.__s2next()', 9000);
    }
    R.final = await c.evaluate(STATE);
    R.detail = await c.evaluate(DETAIL);
    R.exceptions = c.exceptions.slice(0, 20);
  } catch (e) {
    R.notes.push('error: ' + (e && e.message));
  } finally {
    try { c && c.ws.close(); } catch {}
    try { process.kill(root, 'SIGTERM'); } catch {}
    for (let i = 0; i < 50; i++) { if (![...seen].some(p => fs.existsSync(`/proc/${p}`))) break; await sleep(100); }
    for (const p of seen) { try { const cl = fs.readFileSync(`/proc/${p}/cmdline`, 'utf8'); if (cl.includes(profile)) process.kill(p, 'SIGKILL'); } catch {} }
    fs.rmSync(profile, { recursive: true, force: true });
  }
  R.sinkBefore = sinkBefore; R.sinkAfter = defaultSink();
  verdict(R);
  const text = redact(JSON.stringify(R, null, 1));
  fs.writeFileSync(path.join(outDir, `${name}.json`), text);
  const brief = { name, variant, verdict: R.verdict, checks: R.checks, summary: R.summary, notes: R.notes };
  console.log(redact(JSON.stringify(brief)));
}

function verdict(R) {
  const S = R.samples; const F = R.final || {}; const B = R.beforeControls || {};
  const cpts = S.map(s => s.cpt).filter(x => typeof x === 'number');
  const maxCpt = cpts.length ? Math.max(...cpts) : 0;
  const tail = cpts.slice(-3);
  const advancing = tail.length >= 2 && tail[tail.length - 1] > tail[0] + 1;
  const durMs = (R.afterStart?.item?.durationInMillis) || (B.item?.durationInMillis) || 0;
  const usableStatuses = (R.detail?.eme?.statuses && Object.values(R.detail.eme.statuses)) || [];
  const widevine = (R.detail?.eme?.access || []).some(a => a.keySystem === 'com.widevine.alpha' && a.granted);
  R.checks = {
    c1_authorizedWithStorefront: B.isAuthorized === true && !!(B.storefrontId || B.storefrontCountryCode),
    c2_fullSongPast35s: durMs > 60000 && maxCpt > 35 && advancing,
    c3_widevineKeyUsable: widevine && !!R.detail?.eme?.everUsable && usableStatuses.includes('usable'),
    c4_audioToNullSinkOnly: (R.routing?.streams || []).length > 0 && (R.routing?.targets || []).length > 0 && R.routing.targets.every(t => t === NULL_SINK) && R.sinkBefore === R.sinkAfter,
  };
  R.didItRun = { musickitLoaded: !!(R.afterStart && R.afterStart.musickitVersion), configured: !!(R.afterStart && !/loading|loaded|configure-failed|no-page/.test(R.afterStart.phase)), samples: S.length, treeWalked: S.every(s => s.count > 1) };
  const pss = S.filter(s => s.cpt > 1).map(s => s.pssKb);
  R.summary = { maxCurrentPlaybackTime: maxCpt, advancing, durationMs: durMs, isAuthorized: B.isAuthorized, storefront: B.storefrontId, previewOnly: B.previewOnly, item: B.item, pssMedMiB: pss.length ? MiB(median(pss)) : null, rssMedMiB: pss.length ? MiB(median(S.filter(s => s.cpt > 1).map(s => s.rssKb))) : null, procs: S.length ? median(S.map(s => s.count)) : null, firstAudioMs: R.firstAudioMs, firstUsableMs: R.detail?.eme?.firstUsableEpochMs ? R.detail.eme.firstUsableEpochMs - Date.parse(R.startedAt) : null, scheme: R.detail?.mse?.schm, frma: R.detail?.mse?.frma, mseTypes: R.detail?.mse?.types, flavors: R.detail?.flavors, m3u8: R.detail?.m3u8, steps: R.steps };
  const ok = Object.values(R.checks).every(Boolean);
  R.verdict = !R.didItRun.musickitLoaded ? 'VOID' : ok ? 'PASS' : 'FAIL';
}

main();
