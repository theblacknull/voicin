// Issuer logic for one activation request (see docs/activation-protocol.md).
// Dependencies are passed in so the logic can be tested without Redis, RPC or Waku.
import { createHash } from "crypto";

const TX_RE = /^0x[0-9a-fA-F]{64}$/;
const MACHINE_RE = /^[0-9a-f]{64}$/;
const MAX_SKEW_S = 600;

// Returns a plaintext object for the reply: { key, token } on success, or { error } on refusal.
export async function handleRequest(req, deps) {
  const { redis, allKeys, verifyPayment, signToken, networks, treasury, priceUsdt, now } = deps;
  const fail = (error) => ({ error });

  if (!req || req.v !== 1 || req.type !== "activate") return fail("bad_request");
  if (typeof req.reqId !== "string" || typeof req.pubkey !== "string") return fail("bad_request");
  if (!TX_RE.test(req.tx || "")) return fail("bad_request");
  if (!MACHINE_RE.test(req.machine || "")) return fail("bad_request");
  if (!Number.isFinite(req.ts) || Math.abs(now() / 1000 - req.ts) > MAX_SKEW_S) return fail("stale");

  const net = networks[req.network];
  if (!net) return fail("bad_request");

  let paid = false;
  try {
    paid = await verifyPayment(net, treasury, priceUsdt, req.tx);
  } catch {
    return fail("tx_not_confirmed");
  }
  if (!paid) return fail("tx_not_confirmed");

  const claimK = `voicin:claim:${req.tx}`;
  let claim = parse(await redis("GET", claimK));

  if (!claim) {
    // Reserve a key that nobody has used yet (atomic SET NX per key)
    let chosen = null;
    for (const k of allKeys()) {
      if ((await redis("SET", `voicin:used:${k}`, req.tx, "NX")) === "OK") {
        chosen = k;
        break;
      }
    }
    if (!chosen) return fail("sold_out");

    const record = { key: chosen, machine: req.machine, at: now() };
    const saved = await redis("SET", claimK, JSON.stringify(record), "NX");
    if (saved === "OK") {
      claim = record;
    } else {
      // Another request for the same tx won the race: release our reservation, use theirs
      await redis("DEL", `voicin:used:${chosen}`);
      claim = parse(await redis("GET", claimK));
      if (!claim) return fail("bad_request");
    }
  }

  if (claim.machine !== req.machine) return fail("tx_already_used");

  // Bind the key to this machine (same record format as before)
  await redis(
    "SET",
    `voicin:lic:${claim.key}`,
    JSON.stringify({ tx: req.tx, network: req.network, at: claim.at, machine: req.machine }),
  );

  const keyHash = createHash("sha256").update(claim.key).digest("hex");
  return { key: claim.key, token: signToken(keyHash, req.machine) };
}

function parse(raw) {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}
