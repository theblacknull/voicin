// Verifies USDT payments (BSC/ETH) on-chain through JSON-RPC.
const TRANSFER_TOPIC = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const TOLERANCE = 100000n; // small tolerance for rounding

export const NETWORKS = {
  bsc: { rpc: "https://bsc-rpc.publicnode.com", usdt: "0x55d398326f99059ff775485246999027b3197955", decimals: 18 },
  eth: { rpc: "https://ethereum-rpc.publicnode.com", usdt: "0xdac17f958d2ee523a2206206994597c13d831ec7", decimals: 6 },
};

async function rpc(url, method, params) {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    signal: AbortSignal.timeout(10000),
  });
  const body = await res.json();
  if (body.error) throw new Error(JSON.stringify(body.error));
  return body.result;
}

const topicAddr = (addr) => "0x" + addr.replace(/^0x/, "").toLowerCase().padStart(64, "0");

// Returns true if the transaction transfers at least `priceUsdt` USDT to `treasury` on the chosen network
export async function verifyPayment(network, treasury, priceUsdt, txHash) {
  if (!/^0x[0-9a-fA-F]{64}$/.test(txHash)) return false;
  let receipt;
  try {
    receipt = await rpc(network.rpc, "eth_getTransactionReceipt", [txHash]);
  } catch (e) {
    // Public RPCs return "Archive requests" for hashes they have not indexed: treat as not confirmed
    if (String(e.message).includes("Archive")) return false;
    throw e;
  }
  if (!receipt || receipt.status !== "0x1") return false;
  if ((receipt.to || "").toLowerCase() !== network.usdt.toLowerCase()) return false;
  const want = BigInt(priceUsdt) * 10n ** BigInt(network.decimals);
  const expectedTo = topicAddr(treasury);
  for (const log of receipt.logs || []) {
    if ((log.address || "").toLowerCase() !== network.usdt.toLowerCase()) continue;
    const topics = (log.topics || []).map((t) => t.toLowerCase());
    if (topics.length < 3 || topics[0] !== TRANSFER_TOPIC || topics[2] !== expectedTo) continue;
    const amount = BigInt(log.data && log.data !== "0x" ? log.data : "0x0");
    if (amount + TOLERANCE >= want) return true;
  }
  return false;
}
