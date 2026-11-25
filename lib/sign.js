// Signed license tokens (Ed25519). The private key lives only in the server env (LICENSE_SIGNING_KEY).
// The app holds the matching public key and checks tokens offline.
import { createPrivateKey, sign as edSign } from "crypto";

const b64url = (buf) => Buffer.from(buf).toString("base64url");

// Token = base64url(payload JSON) + "." + base64url(signature)
export function issueToken(keyHash, machineHash) {
  const raw = process.env.LICENSE_SIGNING_KEY;
  if (!raw) throw new Error("LICENSE_SIGNING_KEY is not set");
  const priv = createPrivateKey({ key: Buffer.from(raw, "base64"), format: "der", type: "pkcs8" });
  const payload = Buffer.from(JSON.stringify({ v: 1, kh: keyHash, mh: machineHash, iat: Math.floor(Date.now() / 1000) }));
  const sig = edSign(null, payload, priv);
  return `${b64url(payload)}.${b64url(sig)}`;
}
