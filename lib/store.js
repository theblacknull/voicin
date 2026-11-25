// Stores key-issuing state in Redis / Valkey (REDIS_URL).
// The database is shared by several projects, so every key uses a project prefix: "voicin:".
import { createClient } from "redis";

let client = null;

async function getClient() {
  if (!client) {
    const url = process.env.REDIS_URL;
    // Layerbase/Valkey routes by TLS servername: pass the hostname explicitly
    const servername = url ? new URL(url).hostname : undefined;
    client = createClient({ url, socket: { servername, connectTimeout: 10000 } });
    client.on("error", () => {});
    await client.connect();
  }
  return client;
}

// Runs one command. Returns the raw reply ("OK", a string, or null when missing).
export async function redis(...args) {
  const c = await getClient();
  return c.sendCommand(args.map(String));
}

// Key list from the LICENSE_KEYS environment variable (comma-separated)
export function allKeys() {
  return (process.env.LICENSE_KEYS || "")
    .split(",")
    .map((s) => s.trim().toUpperCase())
    .filter(Boolean);
}

// Project prefix: other projects on the same database use their own prefix
const P = "voicin";
export const licKey = (key) => `${P}:lic:${key}`; // binding: {tx, network, at, machine}
export const claimKey = (tx) => `${P}:claim:${tx}`; // payment record: {key, machine, at}
export const usedKey = (key) => `${P}:used:${key}`; // key reserved by a payment tx
