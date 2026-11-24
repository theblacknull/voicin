// Stores key-issuing state in Redis. Vercel's Upstash integration provides REDIS_URL.
import { createClient } from "redis";

let client = null;

async function getClient() {
  if (!client) {
    client = createClient({ url: process.env.REDIS_URL });
    client.on("error", () => {});
    await client.connect();
  }
  return client;
}

// Runs one Redis command. Returns the raw reply ("OK", a string, or null when missing).
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

// Each key is stored at lic:<key> = JSON {tx, network, at, machine}
export const licKey = (key) => `lic:${key}`;
export const claimKey = (tx) => `claim:${tx}`;
