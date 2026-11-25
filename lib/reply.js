// Encrypts a reply so only the app that sent the request can read it.
// X25519 (ephemeral -> app) + HKDF-SHA256 + AES-256-GCM. Public keys are raw 32-byte values, base64.
import { generateKeyPairSync, diffieHellman, hkdfSync, createCipheriv, createDecipheriv, createPublicKey, randomBytes } from "crypto";

const INFO = "voicin-reply-v1";
const SPKI_PREFIX = Buffer.from("302a300506032b656e032100", "hex"); // X25519 SubjectPublicKeyInfo header

// Raw 32-byte X25519 public key -> KeyObject
export function importPublic(rawB64) {
  const raw = Buffer.from(rawB64, "base64");
  if (raw.length !== 32) throw new Error("bad public key");
  return createPublicKey({ key: Buffer.concat([SPKI_PREFIX, raw]), format: "der", type: "spki" });
}

function rawPublic(keyObject) {
  return keyObject.export({ format: "der", type: "spki" }).subarray(-32);
}

function deriveKey(shared) {
  return Buffer.from(hkdfSync("sha256", shared, Buffer.alloc(0), INFO, 32));
}

// Returns the envelope the app expects: { v, eph, nonce, ct }
export function encryptReply(appPubRawB64, plaintext) {
  const appPub = importPublic(appPubRawB64);
  const eph = generateKeyPairSync("x25519");
  const aesKey = deriveKey(diffieHellman({ privateKey: eph.privateKey, publicKey: appPub }));
  const nonce = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", aesKey, nonce);
  const ct = Buffer.concat([cipher.update(JSON.stringify(plaintext), "utf8"), cipher.final(), cipher.getAuthTag()]);
  return {
    v: 1,
    eph: Buffer.from(rawPublic(eph.publicKey)).toString("base64"),
    nonce: nonce.toString("base64"),
    ct: ct.toString("base64"),
  };
}

// Used by the app side (and by tests): decrypt with the app's private KeyObject
export function decryptReply(envelope, appPrivate) {
  const eph = importPublic(envelope.eph);
  const aesKey = deriveKey(diffieHellman({ privateKey: appPrivate, publicKey: eph }));
  const buf = Buffer.from(envelope.ct, "base64");
  const decipher = createDecipheriv("aes-256-gcm", aesKey, Buffer.from(envelope.nonce, "base64"));
  decipher.setAuthTag(buf.subarray(buf.length - 16));
  const text = Buffer.concat([decipher.update(buf.subarray(0, buf.length - 16)), decipher.final()]);
  return JSON.parse(text.toString("utf8"));
}
