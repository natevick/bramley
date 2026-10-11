import { createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import { realpathSync, statSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export const LIFETIME_S = 180 * 86400;
const CHECK_URL = "https://api.music.apple.com/v1/catalog/us/search?term=a&types=songs&limit=1";
const b64u = (b) => Buffer.from(b).toString("base64url");

export function mint({ pem, kid, iss, now = Math.floor(Date.now() / 1000) }) {
  const head = b64u(JSON.stringify({ alg: "ES256", kid }));
  const body = b64u(JSON.stringify({ iss, iat: now, exp: now + LIFETIME_S }));
  const sig = sign("sha256", Buffer.from(`${head}.${body}`), {
    key: createPrivateKey(pem),
    dsaEncoding: "ieee-p1363",
  });
  return { token: `${head}.${body}.${b64u(sig)}`, iat: now, exp: now + LIFETIME_S };
}

export function verifies(token, pem) {
  const [head, body, sig] = token.split(".");
  return verify(
    "sha256",
    Buffer.from(`${head}.${body}`),
    { key: createPublicKey(createPrivateKey(pem)), dsaEncoding: "ieee-p1363" },
    Buffer.from(sig, "base64url"),
  );
}

async function main(out) {
  const env = (name) => {
    const value = process.env[name]?.trim();
    if (!value) throw new Error(`${name} is not set`);
    return value;
  };
  const pem = Buffer.from(env("MUSICKIT_P8_B64"), "base64").toString("utf8");
  const minted = mint({ pem, kid: env("MUSICKIT_KEY_ID"), iss: env("MUSICKIT_TEAM_ID") });
  if (!verifies(minted.token, pem)) throw new Error("the minted token does not verify");
  const reply = await fetch(CHECK_URL, {
    headers: { Authorization: `Bearer ${minted.token}` },
    redirect: "error",
  });
  if (reply.status !== 200) throw new Error(`Apple's catalog answered HTTP ${reply.status}`);
  writeFileSync(out, JSON.stringify(minted));
  if (!(statSync(out).size > 0)) throw new Error(`${out} is empty`);
  console.log(`published token expires ${new Date(minted.exp * 1000).toISOString()}`);
}

if (import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
  main(process.argv[2] ?? "developer-token.json").catch((e) => {
    console.error(`mint failed: ${e.message}`);
    process.exit(1);
  });
}
