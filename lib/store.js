// Stores key-issuing state in Upstash Redis through its REST API (works on Vercel).
// Vercel's Upstash integration sets these variables automatically: UPSTASH_REDIS_REST_URL, UPSTASH_REDIS_REST_TOKEN.
const URL_ = process.env.UPSTASH_REDIS_REST_URL;
const TOKEN = process.env.UPSTASH_REDIS_REST_TOKEN;

export async function redis(...args) {
  const res = await fetch(URL_, {
    method: "POST",
    headers: { Authorization: `Bearer ${TOKEN}`, "content-type": "application/json" },
    body: JSON.stringify(args),
    signal: AbortSignal.timeout(8000),
  });
  const body = await res.json();
  if (body.error) throw new Error(body.error);
  return body.result;
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
