import { send, methodOnly } from "../lib/http.js";

// Purchase info for the website and app: price, receiving wallet, supported networks
export default async function handler(req, res) {
  if (!methodOnly(req, res, "GET")) return;
  send(res, 200, {
    price_usdt: Number(process.env.PRICE_USDT || 10),
    wallet: process.env.TREASURY_EVM,
    networks: ["bsc", "eth"],
  });
}
