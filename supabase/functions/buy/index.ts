// Supabase Edge Function: lifetime license purchase by wallet.
// 1) The buyer gives the wallet address they paid from.
// 2) We scan the chain for USDT transfers from that wallet to our treasury (last ~24 h).
// 3) A new payment reserves one free license (status 'pending').
// 4) Verification is automatic: the chain scan above must show the payment, then the key is issued at once.
// Secrets: LICENSE_KEYS (comma-separated plaintext keys, used only to map a reserved hash back to its key).
import { createClient } from "npm:@supabase/supabase-js@2.45.4";

const TREASURY = "0xb11be01e0d81b20f373aa0de7058946765e68d97";
const PRICE_USDT = 10n;
const TOLERANCE = 100000n;
const TRANSFER_TOPIC = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const NETWORKS: Record<string, { rpc: string; usdt: string; decimals: number; blocksPerDay: number }> = {
  bsc: { rpc: "https://bsc-rpc.publicnode.com", usdt: "0x55d398326f99059ff775485246999027b3197955", decimals: 18, blocksPerDay: 28800 },
  eth: { rpc: "https://ethereum-rpc.publicnode.com", usdt: "0xdac17f958d2ee523a2206206994597c13d831ec7", decimals: 6, blocksPerDay: 7200 },
};
const ADDR_RE = /^0x[0-9a-fA-F]{40}$/;
const TX_RE = /^0x[0-9a-fA-F]{64}$/;
const CHUNK = 5000;
const enc = new TextEncoder();

const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-headers": "content-type, apikey, authorization",
  "access-control-allow-methods": "POST, OPTIONS",
};

function json(obj: unknown, status = 200) {
  return new Response(JSON.stringify(obj), { status, headers: { "content-type": "application/json", ...CORS } });
}

const padAddr = (a: string) => "0x" + a.replace(/^0x/, "").toLowerCase().padStart(64, "0");

async function rpc(url: string, method: string, params: unknown[]) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    signal: AbortSignal.timeout(15000),
  });
  const body = await res.json();
  if (body.error) throw new Error(JSON.stringify(body.error));
  return body.result;
}

// Transfers from payer to treasury of at least 10 USDT in the last ~24 h. Returns tx hashes.
async function findPayments(network: string, payer: string): Promise<string[]> {
  const net = NETWORKS[network];
  const head = BigInt(await rpc(net.rpc, "eth_blockNumber", []));
  const floor = head - BigInt(net.blocksPerDay);
  const minAmount = PRICE_USDT * 10n ** BigInt(net.decimals);
  const found = new Set<string>();
  let to = head;
  while (to > floor) {
    let from = to - BigInt(CHUNK) + 1n;
    if (from < floor) from = floor;
    const logs = await rpc(net.rpc, "eth_getLogs", [{
      fromBlock: "0x" + from.toString(16),
      toBlock: "0x" + to.toString(16),
      address: net.usdt,
      topics: [TRANSFER_TOPIC, padAddr(payer), padAddr(TREASURY)],
    }]);
    for (const l of logs) {
      const amount = BigInt(l.data && l.data !== "0x" ? l.data : "0x0");
      if (amount + TOLERANCE >= minAmount) found.add(String(l.transactionHash).toLowerCase());
    }
    to = from - 1n;
  }
  return [...found];
}

async function sha256Hex(text: string): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", enc.encode(text)));
  return Array.from(d, (b) => b.toString(16).padStart(2, "0")).join("");
}

// Map a license hash back to its plaintext key using LICENSE_KEYS (never stored in the database)
async function keyForHash(keyHash: string): Promise<string | null> {
  const list = (Deno.env.get("LICENSE_KEYS") ?? "").split(",").map((s) => s.trim().toUpperCase()).filter(Boolean);
  for (const k of list) {
    if ((await sha256Hex(k)) === keyHash) return k;
  }
  return null;
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response(null, { status: 204, headers: CORS });
  if (req.method !== "POST") return json({ error: "method not allowed" }, 405);
  const body = await req.json().catch(() => ({} as Record<string, unknown>));
  const network = String(body.network ?? "");
  const payer = String(body.payer ?? "").trim();
  const tx = body.tx ? String(body.tx).trim().toLowerCase() : null;
  if (!NETWORKS[network]) return json({ error: "unsupported network" }, 400);
  if (!ADDR_RE.test(payer)) return json({ error: "invalid wallet address" }, 400);
  if (tx !== null && !TX_RE.test(tx)) return json({ error: "invalid transaction hash" }, 400);

  const db = createClient(Deno.env.get("SUPABASE_URL")!, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!, {
    auth: { persistSession: false },
  });

  let txs: string[];
  try {
    txs = await findPayments(network, payer);
  } catch {
    return json({ error: "could not read the blockchain, try again" }, 502);
  }
  // A tx hash from the buyer is only used if the chain confirms it is a valid payment from this wallet
  if (tx !== null) txs = txs.filter((t) => t === tx);
  if (txs.length === 0) return json({ status: "not_found" });

  for (const t of txs) {
    const { data: existing, error: selErr } = await db.from("orders").select("*").eq("tx", t).maybeSingle();
    if (selErr) return json({ error: "server error" }, 500);

    if (existing) {
      if (existing.payer.toLowerCase() !== payer.toLowerCase()) return json({ error: "transaction belongs to another wallet" }, 403);
      if (existing.status !== "confirmed") return json({ status: "pending", tx: t });
      const key = await keyForHash(existing.key_hash);
      return key ? json({ status: "confirmed", tx: t, key }) : json({ error: "server error" }, 500);
    }

    // New payment: reserve one license for it
    const { data: reserved, error: rpcErr } = await db.rpc("reserve_license", { p_tx: t });
    if (rpcErr) return json({ error: "server error" }, 500);
    if (!reserved) return json({ error: "sold out" }, 409);

    // The chain check above already verified: USDT transfer from this wallet to the treasury, >= 10 USDT, confirmed.
    const { error: insErr } = await db.from("orders").insert({
      tx: t, network, payer: payer.toLowerCase(), key_hash: reserved, status: "confirmed", confirmed_at: new Date().toISOString(),
    });
    if (insErr) return json({ error: "server error" }, 500);
    const key = await keyForHash(reserved);
    return key ? json({ status: "confirmed", tx: t, key }) : json({ error: "server error" }, 500);
  }
  return json({ status: "not_found" });
});
