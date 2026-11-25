import { createHash } from "crypto";
import { send, body, methodOnly } from "../lib/http.js";
import { redis, allKeys, licKey } from "../lib/store.js";
import { issueToken } from "../lib/sign.js";

// Called by the app once, when a key is entered. Binds the key to the first device and returns a signed token.
// The app verifies the token offline, so it does not call this again.
export default async function handler(req, res) {
  if (!methodOnly(req, res, "POST")) return;
  const { key: rawKey, machine: rawMachine } = body(req);
  const key = String(rawKey || "").trim().toUpperCase();
  const machine = String(rawMachine || "").trim();
  if (!allKeys().includes(key)) return send(res, 404, { error: "invalid license key" });
  if (!machine || machine.length > 128) return send(res, 400, { error: "invalid device id" });

  const raw = await redis("GET", licKey(key));
  if (!raw) {
    // Key is in the list but was never sold through a transaction (issued by hand): bind it to this device
    const info = { tx: "manual", network: "-", at: Date.now(), machine };
    await redis("SET", licKey(key), JSON.stringify(info), "NX");
  } else {
    const info = JSON.parse(raw);
    if (!info.machine) {
      info.machine = machine;
      await redis("SET", licKey(key), JSON.stringify(info));
    } else if (info.machine !== machine) {
      return send(res, 403, { error: "this key is already used on another device" });
    }
  }

  const keyHash = createHash("sha256").update(key).digest("hex");
  return send(res, 200, { ok: true, token: issueToken(keyHash, machine) });
}
