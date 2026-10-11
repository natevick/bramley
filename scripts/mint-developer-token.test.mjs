import { generateKeyPairSync } from "node:crypto";
import assert from "node:assert/strict";
import test from "node:test";

import { LIFETIME_S, mint, verifies } from "./mint-developer-token.mjs";

const { privateKey } = generateKeyPairSync("ec", { namedCurve: "P-256" });
const pem = privateKey.export({ type: "pkcs8", format: "pem" });
const part = (token, i) => JSON.parse(Buffer.from(token.split(".")[i], "base64url"));

test("a throwaway-key mint verifies with 180-day claims and no origin", () => {
  const minted = mint({ pem, kid: "KID0000000", iss: "TEAM000000", now: 1_700_000_000 });
  assert.deepEqual(part(minted.token, 0), { alg: "ES256", kid: "KID0000000" });
  assert.deepEqual(part(minted.token, 1), {
    iss: "TEAM000000",
    iat: 1_700_000_000,
    exp: 1_700_000_000 + LIFETIME_S,
  });
  assert.equal(LIFETIME_S, 180 * 86400);
  assert.deepEqual([minted.iat, minted.exp], [1_700_000_000, 1_700_000_000 + LIFETIME_S]);
  assert.equal(Buffer.from(minted.token.split(".")[2], "base64url").length, 64);
  assert.ok(verifies(minted.token, pem));
});

test("a token with one flipped signature byte or another key fails verification", () => {
  const { token } = mint({ pem, kid: "KID0000000", iss: "TEAM000000" });
  const [head, body, sig] = token.split(".");
  const bytes = Buffer.from(sig, "base64url");
  bytes[0] ^= 1;
  assert.equal(verifies(`${head}.${body}.${bytes.toString("base64url")}`, pem), false);
  const other = generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey;
  assert.equal(verifies(token, other.export({ type: "pkcs8", format: "pem" })), false);
});
