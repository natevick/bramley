import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const CHROME = '/opt/google/chrome/chrome';
const NULL_SINK = 'spike1-null';
const RUNTIME_DIR = '/run/user/' + process.getuid();

function parseArgs(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith('--')) { o[argv[i].slice(2)] = argv[i + 1]; i++; }
  }
  return o;
}

const opt = parseArgs(process.argv.slice(2));
const name = opt.name;
const cfg = JSON.parse(fs.readFileSync(opt.config, 'utf8'));
const mode = opt.mode || 'drm';
const warmup = Number(opt.warmup ?? 15);
const sampleSecs = Number(opt.sample ?? 60);
const pauseSecs = Number(opt.pause ?? 0);
const firstAudioTimeout = Number(opt['first-audio-timeout'] ?? 60);
const pagePort = Number(opt.port ?? 8765);
const lic = opt.lic || 'https://cwip-shaka-proxy.appspot.com/no_auth';
const mpd = opt.mpd || 'https://storage.googleapis.com/shaka-demo-assets/sintel-widevine/dash.mpd';
const outDir = path.join(HERE, 'results');
fs.mkdirSync(outDir, { recursive: true });

const log = (...a) => { if (process.env.SPIKE_DEBUG) console.error('[h]', ...a); };
const sleep = ms => new Promise(r => setTimeout(r, ms));
const MiB = kb => Math.round((kb / 1024) * 10) / 10;
const median = a => { const s = [...a].sort((x, y) => x - y); const m = Math.floor(s.length / 2); return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };

function buildArgs(profile) {
  const a = ['--headless=new', '--no-first-run', '--no-default-browser-check', '--autoplay-policy=no-user-gesture-required'];
  a.push(...(cfg.args || []));
  if (cfg.enable && cfg.enable.length) a.push('--enable-features=' + cfg.enable.join(','));
  if (cfg.disable && cfg.disable.length) a.push('--disable-features=' + cfg.disable.join(','));
  if (cfg.jsFlags && cfg.jsFlags.length) a.push('--js-flags=' + cfg.jsFlags.join(' '));
  a.push('--user-data-dir=' + profile, '--remote-debugging-port=0');
  const url = new URL(`http://127.0.0.1:${pagePort}/page.html`);
  url.searchParams.set('mode', mode);
  url.searchParams.set('lic', lic);
  url.searchParams.set('mpd', mpd);
  a.push(url.toString());
  return a;
}

function readProc() {
  const procs = new Map();
  for (const d of fs.readdirSync('/proc')) {
    if (!/^\d+$/.test(d)) continue;
    try {
      const stat = fs.readFileSync(`/proc/${d}/stat`, 'utf8');
      const rest = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
      procs.set(Number(d), { ppid: Number(rest[1]) });
    } catch {}
  }
  return procs;
}

function cmdline(pid) {
  try { return fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean); } catch { return null; }
}

function tree(rootPid, tag) {
  const procs = readProc();
  const kids = new Map();
  for (const [pid, p] of procs) { if (!kids.has(p.ppid)) kids.set(p.ppid, []); kids.get(p.ppid).push(pid); }
  const out = new Set();
  const stack = [rootPid];
  while (stack.length) { const p = stack.pop(); if (!procs.has(p) || out.has(p)) continue; out.add(p); for (const c of kids.get(p) || []) stack.push(c); }
  for (const pid of procs.keys()) {
    if (out.has(pid)) continue;
    const c = cmdline(pid);
    if (c && c.some(x => x.includes(tag))) out.add(pid);
  }
  return [...out];
}

function procType(pid) {
  const c = (cmdline(pid) || []).join(' ');
  const t = c.match(/--type=(\S+)/);
  const st = c.match(/--utility-sub-type=(\S+)/);
  if (!t) return c.startsWith(CHROME) ? 'browser' : (c.split(' ')[0] || '?');
  return t[1] + (st ? ':' + st[1] : '');
}

function rollup(pid) {
  try {
    const r = { Rss: 0, Pss: 0, Private_Clean: 0, Private_Dirty: 0, SwapPss: 0 };
    for (const line of fs.readFileSync(`/proc/${pid}/smaps_rollup`, 'utf8').split('\n')) {
      const m = line.match(/^(\w+):\s+(\d+) kB/);
      if (m && m[1] in r) r[m[1]] = Number(m[2]);
    }
    return r;
  } catch { return null; }
}

function sampleMem(rootPid, tag) {
  const pids = tree(rootPid, tag);
  let pss = 0, rss = 0, uss = 0, swap = 0;
  const per = [];
  for (const pid of pids) {
    const r = rollup(pid);
    if (!r) continue;
    pss += r.Pss; rss += r.Rss; uss += r.Private_Clean + r.Private_Dirty; swap += r.SwapPss;
    per.push({ pid, type: procType(pid), pssMiB: MiB(r.Pss), rssMiB: MiB(r.Rss), ussMiB: MiB(r.Private_Clean + r.Private_Dirty) });
  }
  return { pids, count: per.length, pssKb: pss, rssKb: rss, ussKb: uss, swapPssKb: swap, per };
}

function pwRouting(pids) {
  try {
    const dump = JSON.parse(execFileSync('pw-dump', { env: { ...process.env, XDG_RUNTIME_DIR: RUNTIME_DIR }, maxBuffer: 64 << 20 }).toString());
    const nodes = new Map();
    for (const o of dump) if (o.type === 'PipeWire:Interface:Node') nodes.set(o.id, o.info?.props || {});
    const ours = [...nodes].filter(([, p]) => pids.includes(Number(p['application.process.id'])));
    const targets = [];
    for (const o of dump) {
      if (o.type !== 'PipeWire:Interface:Link') continue;
      const from = o.info?.['output-node-id'];
      if (ours.some(([id]) => id === from)) targets.push(nodes.get(o.info['input-node-id'])?.['node.name']);
    }
    return { streams: ours.map(([id, p]) => ({ id, name: p['node.name'], app: p['application.name'], pid: p['application.process.id'] })), targets: [...new Set(targets)] };
  } catch (e) { return { error: String(e) }; }
}

async function cdpConnect(profile) {
  const portFile = path.join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 200 && !fs.existsSync(portFile); i++) await sleep(100);
  const port = fs.readFileSync(portFile, 'utf8').split('\n')[0].trim();
  let page;
  for (let i = 0; i < 100 && !page; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      page = list.find(t => t.type === 'page' && t.url.includes('page.html'));
    } catch {}
    if (!page) await sleep(100);
  }
  log('page target', page && page.webSocketDebuggerUrl);
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  log('ws open');
  let id = 0;
  const pending = new Map();
  ws.onmessage = ev => { const m = JSON.parse(ev.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
  const evaluate = expression => new Promise(res => {
    const i = ++id; pending.set(i, m => res(m.result?.result?.value));
    ws.send(JSON.stringify({ id: i, method: 'Runtime.evaluate', params: { expression, returnByValue: true } }));
  });
  return { ws, evaluate };
}

const STATE_EXPR = `(() => { const a = window.__spikeAudio; const s = window.__spike || {}; return { phase: s.phase, errors: s.errors, everUsable: s.everUsable, keyStatuses: s.keyStatuses, sessions: s.sessions, keySystem: s.keySystem, robustness: s.robustness, licenseRequests: s.licenseRequests, licenseResponses: s.licenseResponses, sourceBuffers: s.sourceBuffers, audioOnly: s.audioOnly, activeVariant: s.activeVariant, configureOk: s.configureOk, musickitLoaded: s.musickitLoaded || (typeof MusicKit !== 'undefined'), firstAudioEpochMs: s.firstAudioEpochMs, firstUsableEpochMs: s.firstUsableEpochMs, playerKeySystem: window.__spikePlayer ? window.__spikePlayer.keySystem() : null, currentTime: a ? a.currentTime : null, paused: a ? a.paused : null, readyState: a ? a.readyState : null }; })()`;

async function main() {
  const stamp = new Date().toISOString().replace(/[:.]/g, '-');
  const profile = path.join(HERE, 'profiles', `${name}-${stamp}`);
  fs.mkdirSync(profile, { recursive: true });
  const args = buildArgs(profile);
  const logFile = fs.openSync(path.join(outDir, `${name}.chrome.log`), 'w');
  const tSpawn = Date.now();
  const child = spawn(CHROME, args, { env: { ...process.env, XDG_RUNTIME_DIR: RUNTIME_DIR, PULSE_SINK: NULL_SINK }, stdio: ['ignore', logFile, logFile] });
  const rootPid = child.pid;
  const seen = new Set([rootPid]);
  const result = { name, mode, cfg, args, warmup, sampleSecs, pauseSecs, lic, mpd, startedAt: new Date(tSpawn).toISOString(), samples: [], pauseSamples: [], checks: {}, notes: [] };
  let cdp;
  try {
    cdp = await cdpConnect(profile);
    let st;
    const deadline = Date.now() + firstAudioTimeout * 1000;
    while (Date.now() < deadline) {
      st = await cdp.evaluate(STATE_EXPR);
      log('state', JSON.stringify(st));
      if (st && (st.firstAudioEpochMs || st.phase === 'failed')) break;
      await sleep(100);
    }
    result.firstAudioMs = st && st.firstAudioEpochMs ? st.firstAudioEpochMs - tSpawn : null;
    log('first-audio loop done');
    const t0 = sampleMem(rootPid, profile);
    log('t0 sampled', t0.count);
    t0.pids.forEach(p => seen.add(p));
    result.routing = pwRouting(t0.pids);
    log('routing', JSON.stringify(result.routing));
    if (st && st.firstAudioEpochMs && !(result.routing.targets || []).every(t => t === NULL_SINK)) {
      result.notes.push('ABORT: audio not routed only to null sink: ' + JSON.stringify(result.routing));
      throw new Error('routing');
    }
    if (mode === 'silence') { result.state = st; }
    else {
    const noAudio = !(st && st.firstAudioEpochMs);
    if (noAudio) result.notes.push('no first audio within ' + firstAudioTimeout + 's');
    const wu = noAudio ? 0 : warmup;
    for (let t = 0; t < wu; t += 2) await sleep(2000);
    const n = noAudio ? Math.min(5, Math.floor(sampleSecs / 2)) : Math.floor(sampleSecs / 2);
    for (let i = 0; i <= n; i++) {
      const m = sampleMem(rootPid, profile);
      m.pids.forEach(p => seen.add(p));
      const s = await cdp.evaluate(STATE_EXPR);
      result.samples.push({ t: Date.now() - tSpawn, count: m.count, pssKb: m.pssKb, rssKb: m.rssKb, ussKb: m.ussKb, swapPssKb: m.swapPssKb, currentTime: s?.currentTime, paused: s?.paused, everUsable: s?.everUsable, types: m.per.map(p => p.type) });
      if (i === Math.floor(n / 2)) { result.midRouting = pwRouting(m.pids); result.midPer = m.per; }
      if (i < n) await sleep(2000);
    }
    result.state = await cdp.evaluate(STATE_EXPR);
    if (pauseSecs > 0 && !noAudio) {
      await cdp.evaluate('window.__spikePause()');
      for (let i = 0; i <= Math.floor(pauseSecs / 2); i++) {
        const m = sampleMem(rootPid, profile);
        const s = await cdp.evaluate(STATE_EXPR);
        result.pauseSamples.push({ count: m.count, pssKb: m.pssKb, rssKb: m.rssKb, ussKb: m.ussKb, currentTime: s?.currentTime, paused: s?.paused, types: m.per.map(p => p.type) });
        if (i < Math.floor(pauseSecs / 2)) await sleep(2000);
      }
      result.pauseRouting = pwRouting(sampleMem(rootPid, profile).pids);
    }
    }
  } catch (e) {
    result.notes.push('error: ' + (e && e.message));
  } finally {
    try { cdp && cdp.ws.close(); } catch {}
    log('finally');
    const browserCmd = cmdline(rootPid);
    result.browserCmdline = browserCmd;
    try { process.kill(rootPid, 'SIGTERM'); } catch {}
    for (let i = 0; i < 50; i++) { if (![...seen].some(p => cmdline(p))) break; await sleep(100); }
    for (const p of seen) { const c = cmdline(p); if (c && c.some(x => x.includes('chrome'))) { try { process.kill(p, 'SIGKILL'); } catch {} } }
    log('killed');
    fs.rmSync(profile, { recursive: true, force: true });
    log('rm done');
  }
  result.firstUsableMs = result.state && result.state.firstUsableEpochMs ? result.state.firstUsableEpochMs - tSpawn : null;
  summarize(result);
  fs.writeFileSync(path.join(outDir, `${name}.json`), JSON.stringify(result, null, 1));
  const s = result.summary || {};
  console.log(JSON.stringify({ name, verdict: result.verdict, firstAudioMs: result.firstAudioMs, firstUsableMs: result.firstUsableMs, ...s, checks: result.checks, notes: result.notes }));
}

function summarize(r) {
  if (r.mode === 'silence') { r.verdict = 'ROUTING-CHECK'; r.summary = { routing: r.routing, phase: r.state?.phase }; return; }
  const S = r.samples;
  const ct = S.map(s => s.currentTime ?? 0);
  const st = r.state || {};
  r.checks = {
    c1_playedPast30s: S.length > 1 && ct[ct.length - 1] > 30 && ct[ct.length - 1] - ct[0] >= Math.max(1, r.sampleSecs - 10),
    c2_keyUsable: !!st.everUsable && Object.values(st.keyStatuses || {}).includes('usable'),
    c3_audioOnly: st.audioOnly === true && (st.sourceBuffers || []).length > 0 && (st.sourceBuffers || []).every(m => m.startsWith('audio/')),
    c4_audioOutput: (r.midRouting?.streams || []).length > 0 && (r.midRouting?.targets || []).includes(NULL_SINK),
    c5_treeWalked: S.length > 0 && S.every(s => s.count > 1)
  };
  r.checks.detail = { ctStart: ct[0], ctEnd: ct[ct.length - 1], keyStatuses: st.keyStatuses, keySystem: st.keySystem, playerKeySystem: st.playerKeySystem, robustness: st.robustness, sourceBuffers: st.sourceBuffers, activeVariant: st.activeVariant, licenseRequests: st.licenseRequests, licenseResponses: st.licenseResponses, errors: st.errors, musickitLoaded: st.musickitLoaded, configureOk: st.configureOk, midRouting: r.midRouting };
  const ok = ['c1_playedPast30s', 'c2_keyUsable', 'c3_audioOnly', 'c4_audioOutput', 'c5_treeWalked'].every(k => r.checks[k]);
  const pss = S.map(s => s.pssKb), rss = S.map(s => s.rssKb), uss = S.map(s => s.ussKb);
  r.summary = S.length ? {
    samples: S.length,
    pssMedMiB: MiB(median(pss)), pssMaxMiB: MiB(Math.max(...pss)),
    rssMedMiB: MiB(median(rss)), rssMaxMiB: MiB(Math.max(...rss)),
    ussMedMiB: MiB(median(uss)), ussMaxMiB: MiB(Math.max(...uss)),
    procCount: median(S.map(s => s.count)),
    types: S[S.length - 1].types
  } : {};
  if (r.pauseSamples.length) {
    const P = r.pauseSamples;
    r.summary.paused = { pssMedMiB: MiB(median(P.map(p => p.pssKb))), rssMedMiB: MiB(median(P.map(p => p.rssKb))), ussMedMiB: MiB(median(P.map(p => p.ussKb))), procCount: median(P.map(p => p.count)), stillPaused: P[P.length - 1].paused, ctFrozen: P[P.length - 1].currentTime - P[0].currentTime < 0.5, streamsWhilePaused: (r.pauseRouting?.streams || []).length };
  }
  if (!ok) { r.verdict = 'VOID'; return; }
  const p = r.summary.pssMedMiB;
  r.verdict = p <= 150 ? 'PASS' : p <= 200 ? 'MARGINAL' : 'FAIL';
}

main().then(() => process.exit(0), e => { console.error(e); process.exit(1); });
