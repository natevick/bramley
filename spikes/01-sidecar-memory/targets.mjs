import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const HERE = path.dirname(new URL(import.meta.url).pathname);
const profile = path.join(HERE, 'profiles', 'targets-' + Date.now());
fs.mkdirSync(profile, { recursive: true });
const extra = process.argv.slice(2);
const c = spawn('/opt/google/chrome/chrome', ['--headless=new', '--no-first-run', '--no-default-browser-check', '--user-data-dir=' + profile, '--remote-debugging-port=0', ...extra], { stdio: 'ignore', env: { ...process.env, XDG_RUNTIME_DIR: '/run/user/' + process.getuid(), PULSE_SINK: 'spike1-null' } });
function ppid(pid) { try { const st = fs.readFileSync(`/proc/${pid}/stat`, 'utf8'); return Number(st.slice(st.lastIndexOf(')') + 2).split(' ')[1]); } catch { return 0; } }
function isDesc(pid) { let p = pid; for (let i = 0; i < 10 && p > 1; i++) { p = ppid(p); if (p === c.pid) return true; } return false; }
const sleep = ms => new Promise(r => setTimeout(r, ms));
const pf = path.join(profile, 'DevToolsActivePort');
for (let i = 0; i < 100 && !fs.existsSync(pf); i++) await sleep(100);
await sleep(Number(process.env.PROBE_WAIT || 6000));
const port = fs.readFileSync(pf, 'utf8').split('\n')[0];
const ws = new WebSocket((await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()).webSocketDebuggerUrl);
await new Promise(r => ws.onopen = r);
const res = await new Promise(r => { ws.onmessage = e => r(JSON.parse(e.data)); ws.send(JSON.stringify({ id: 1, method: 'Target.getTargets' })); });
for (const t of res.result.targetInfos) console.log(t.type, t.url.slice(0, 90), t.attached);
ws.close();
const types = [];
for (const d of fs.readdirSync('/proc')) { if (!/^\d+$/.test(d)) continue; try { const cl = fs.readFileSync(`/proc/${d}/cmdline`, 'utf8').replace(/\0/g, ' '); if (cl.includes(profile) || cl.includes('--type=')) { const m = cl.match(/--utility-sub-type=(\S+)/) || cl.match(/--type=(\S+)/); const st = fs.readFileSync(`/proc/${d}/stat`, 'utf8'); if (cl.startsWith('/opt/google/chrome/chrome') && (cl.includes(profile) || isDesc(Number(d)))) types.push(m ? m[1] : 'browser'); } } catch {} }
console.log('PROCS', types.sort().join(' '));
c.kill('SIGTERM');
await sleep(1500);
fs.rmSync(profile, { recursive: true, force: true });
process.exit(0);
