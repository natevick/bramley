import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const SOURCE = process.env.TRACE_UNDER_TEST || new URL("./trace.js", import.meta.url).pathname;
const SECRETS = ["SECRETQUERY", "token", "hunter2", "alice", "Favourite", "Lazarus", "abc-song-name"];
const rejected = name => `Promise.reject(Object.assign(new Error("Lazarus Song Title"), { name: "${name}" }))`;

function traced() {
  const out = [], ctx = vm.createContext({ console: { log: line => out.push(line) } });
  vm.runInContext(`
    class EventTarget { addEventListener() {} removeEventListener() {} }
    class TimeRanges {}
    class MediaSource extends EventTarget {}
    class SourceBuffer extends EventTarget { appendBuffer() {} }
    class SourceBufferList extends EventTarget {}
    class HTMLMediaElement extends EventTarget {}
    Object.defineProperty(HTMLMediaElement.prototype, "src", { configurable: true, get() { return this._src; }, set(v) { this._src = v; } });
    class XMLHttpRequest extends EventTarget { open() {} send() {} abort() {} }
    class MediaKeySystemAccess {}
    class MediaKeys {}
    class MediaKeySession extends EventTarget {
      generateRequest() {}
      update() { return ${rejected("NotAllowedError")}; }
      remove() { return ${rejected("Lazarus")}; }
      load() {}
    }
    Object.assign(globalThis, { EventTarget, TimeRanges, MediaSource, SourceBuffer, SourceBufferList, HTMLMediaElement, XMLHttpRequest, MediaKeySystemAccess, MediaKeys, MediaKeySession,
      performance: { now: () => 1 }, navigator: { requestMediaKeySystemAccess: () => Promise.resolve({}) },
      fetch: () => Promise.resolve({ status: 200, headers: { get: h => h === "etag" ? "abc-song-name" : "321" } }),
      Response: class { arrayBuffer() {} json() {} text() {} } });
  `, ctx);
  vm.runInContext(fs.readFileSync(SOURCE, "utf8"), ctx);
  return { out, run: code => vm.runInContext(code, ctx), leaked: () => SECRETS.filter(s => out.join("\n").includes(s)) };
}

test("a URL logs its host and a hash, never its query, fragment or userinfo", () => {
  const t = traced();
  t.run(`const x = new XMLHttpRequest();
    x.open("GET", "https://cdn.example?token=SECRETQUERY"); x.open("GET", "https://cdn.example#token=SECRETQUERY");
    x.open("GET", "https://alice:hunter2@cdn.example/a?token=SECRETQUERY"); fetch("https://alice:hunter2@cdn.example");`);
  assert.equal(t.out.filter(l => l.includes("cdn.example~")).length, 4);
  assert.deepEqual(t.leaked(), []);
});

test("a name-like string, header value or error message is logged as a length", async () => {
  const t = traced();
  t.run(`new HTMLMediaElement().src = "My Favourite Song Title"; new MediaKeySession().load("Lazarus");
    new MediaKeySession().update(1).catch(() => {}); new MediaKeySession().remove().catch(() => {}); fetch("https://cdn.example/a");`);
  await new Promise(r => setTimeout(r, 5));
  for (const want of ["str23", "rejected NotAllowedError msg18", "rejected err msg18", "etag=str13", "content-length=321"]) assert.ok(t.out.some(l => l.includes(want)), want);
  assert.deepEqual(t.leaked(), []);
});

test("an allowlisted token, a buffer length and a step mark are logged", () => {
  const t = traced();
  t.run(`new MediaKeySession().generateRequest("cenc", new ArrayBuffer(8)); __traceMark("seek", "start"); __traceMark("Favourite");`);
  assert.ok(t.out.some(l => l.includes("generateRequest cenc,ab8")));
  assert.ok(t.out.some(l => l.endsWith("mark seek start")) && t.out.some(l => l.endsWith("mark str9")));
  assert.deepEqual(t.leaked(), []);
});
