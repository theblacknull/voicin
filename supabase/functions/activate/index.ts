// Supabase Edge Function: activates a Voicin license key for one machine and returns a signed token.
// Secrets (set in Supabase, never in the app):
//   LICENSE_SIGNING_KEY  base64 PKCS#8 Ed25519 private key (same key as before)
// SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY are provided by Supabase automatically.
import { createClient } from "npm:@supabase/supabase-js@2.45.4";

const enc = new TextEncoder();
const MACHINE_RE = /^[0-9a-f]{64}$/;

function json(obj: unknown, status: number) {
  return new Response(JSON.stringify(obj), { status, headers: { "content-type": "application/json" } });
}

function b64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function sha256Hex(text: string): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", enc.encode(text)));
  return Array.from(d, (b) => b.toString(16).padStart(2, "0")).join("");
}

// Token = base64url(payload JSON) + "." + base64url(Ed25519 signature)
async function signToken(keyHash: string, machine: string): Promise<string> {
  const der = Uint8Array.from(atob(Deno.env.get("LICENSE_SIGNING_KEY") ?? ""), (c) => c.charCodeAt(0));
  const priv = await crypto.subtle.importKey("pkcs8", der, { name: "Ed25519" }, false, ["sign"]);
  const payload = enc.encode(JSON.stringify({ v: 1, kh: keyHash, mh: machine, iat: Math.floor(Date.now() / 1000) }));
  const sig = new Uint8Array(await crypto.subtle.sign("Ed25519", priv, payload));
  return `${b64url(payload)}.${b64url(sig)}`;
}

Deno.serve(async (req) => {
  if (req.method !== "POST") return json({ error: "method not allowed" }, 405);

  const body = await req.json().catch(() => ({} as Record<string, unknown>));
  const key = String(body.key ?? "").trim().toUpperCase();
  const machine = String(body.machine ?? "").trim();
  if (!key) return json({ error: "invalid license key" }, 404);
  if (!MACHINE_RE.test(machine)) return json({ error: "invalid device id" }, 400);

  const keyHash = await sha256Hex(key);
  const db = createClient(Deno.env.get("SUPABASE_URL")!, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!, {
    auth: { persistSession: false },
  });

  const { data: row, error } = await db.from("licenses").select("key_hash, machine").eq("key_hash", keyHash).maybeSingle();
  if (error) return json({ error: "server error" }, 500);
  if (!row) return json({ error: "invalid license key" }, 404);

  if (row.machine === null) {
    // Atomic bind: only succeeds if the key is still unbound
    const { data: bound, error: upErr } = await db
      .from("licenses")
      .update({ machine, bound_at: new Date().toISOString() })
      .eq("key_hash", keyHash)
      .is("machine", null)
      .select("key_hash");
    if (upErr) return json({ error: "server error" }, 500);
    if (!bound || bound.length === 0) {
      // Someone else bound it between our read and update: re-check
      const { data: again } = await db.from("licenses").select("machine").eq("key_hash", keyHash).maybeSingle();
      if (!again || again.machine !== machine) return json({ error: "this key is already used on another device" }, 403);
    }
  } else if (row.machine !== machine) {
    return json({ error: "this key is already used on another device" }, 403);
  }

  return json({ ok: true, token: await signToken(keyHash, machine) }, 200);
});
