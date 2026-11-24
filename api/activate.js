import { send, body, methodOnly } from "../lib/http.js";
import { redis, allKeys, licKey } from "../lib/store.js";

// Called by the app on launch and when a key is entered. Binds the key to the first device; other devices are refused.
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
    return send(res, 200, { ok: true });
  }
  const info = JSON.parse(raw);
  if (!info.machine) {
    info.machine = machine;
    await redis("SET", licKey(key), JSON.stringify(info));
    return send(res, 200, { ok: true });
  }
  if (info.machine !== machine) {
    return send(res, 403, { error: "this key is already used on another device" });
  }
  return send(res, 200, { ok: true });
}
