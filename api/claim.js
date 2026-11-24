import { send, body, methodOnly } from "../lib/http.js";
import { NETWORKS, verifyPayment } from "../lib/chain.js";
import { redis, allKeys, licKey, claimKey } from "../lib/store.js";

// The buyer sends a USDT transaction hash. The server checks it on-chain, then issues one unused key.
export default async function handler(req, res) {
  if (!methodOnly(req, res, "POST")) return;
  const { network, tx_hash } = body(req);
  const net = NETWORKS[network];
  if (!net) return send(res, 400, { error: "unsupported network" });
  const tx = String(tx_hash || "").trim().toLowerCase();
  if (!/^0x[0-9a-f]{64}$/.test(tx)) return send(res, 400, { error: "invalid transaction hash" });

  // If this transaction already received a key, return the same key (safe to retry)
  const existing = await redis("GET", claimKey(tx));
  if (existing && existing !== "pending") return send(res, 200, { key: existing });

  let paid;
  try {
    paid = await verifyPayment(net, process.env.TREASURY_EVM, Number(process.env.PRICE_USDT || 10), tx);
  } catch {
    return send(res, 502, { error: "could not reach the blockchain, try again" });
  }
  if (!paid) {
    return send(res, 402, { error: "no confirmed payment of the full price to the wallet on this network" });
  }

  // Lock the transaction so two simultaneous requests cannot get two keys
  const lock = await redis("SET", claimKey(tx), "pending", "NX", "EX", 120);
  if (lock !== "OK") {
    const again = await redis("GET", claimKey(tx));
    if (again && again !== "pending") return send(res, 200, { key: again });
    return send(res, 409, { error: "payment is being processed, try again in a moment" });
  }

  // Reserve an unused key (SET NX: each key is issued only once)
  for (const key of allKeys()) {
    const info = { tx, network, at: Date.now(), machine: null };
    const ok = await redis("SET", licKey(key), JSON.stringify(info), "NX");
    if (ok === "OK") {
      await redis("SET", claimKey(tx), key);
      return send(res, 200, { key });
    }
  }
  await redis("DEL", claimKey(tx));
  return send(res, 503, { error: "no license keys left, contact the seller" });
}
