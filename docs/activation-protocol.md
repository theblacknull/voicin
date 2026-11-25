# Voicin activation protocol (Waku + on-chain payment)

Goal: a buyer pays USDT once, then the app activates itself with no Vercel endpoint in the path.
Each payment yields one key, bound to one machine. The app verifies the signed token offline afterwards.

## Parties

- **App**: sends requests and receives replies over the Waku REST API (no Waku library in the app).
- **Issuer**: an always-available worker that reads requests, verifies the payment on chain, and replies.
  It runs on Vercel (API routes) with state in Redis (`REDIS_URL`, already in use).
- **Waku nodes**: public nwaku nodes reached over HTTPS. They relay messages only; they hold no secrets.

## Keys and identities

| Item | Where it lives | Purpose |
| --- | --- | --- |
| Issuer signing key (Ed25519 private) | Issuer env `LICENSE_SIGNING_KEY` | Signs license tokens |
| Ed25519 public key | App source (`license_pub.txt`) | Verifies tokens offline |
| App encryption key (X25519) | App data folder, generated on first run | Lets the issuer encrypt replies to this install only |
| Machine hash | Derived from hardware UUID | Binds the key to one PC |

## Waku topics

- Pubsub topic: the default shard the chosen nodes serve (for example `/waku/2/rs/1/0`). Fix it in config.
- Request content topic: `/voicin/1/req/proto`
- Reply content topic: `/voicin/1/res/<reqId>`, where `reqId` is a random 16-byte hex value chosen by the app.
  A fresh topic per request keeps replies private to that app instance.

## Messages

All payloads are UTF-8 JSON, base64-encoded in the Waku message.

### Request (app → issuer, content topic `/voicin/1/req/proto`)

```json
{
  "v": 1,
  "type": "activate",
  "reqId": "9f1c...",
  "tx": "0x<payment tx hash>",
  "network": "bsc",
  "machine": "<machine hash hex>",
  "pubkey": "<app X25519 public key, base64>",
  "ts": 1795600000
}
```

Rules the issuer enforces:
- `ts` must be within 10 minutes of the issuer's clock.
- `tx` must match `^0x[0-9a-fA-F]{64}$`.
- `machine` must be 64 hex characters.

### Reply (issuer → app, content topic `/voicin/1/res/<reqId>`)

Encrypted so only the app can read it:

```json
{
  "v": 1,
  "eph": "<issuer ephemeral X25519 public key, base64>",
  "nonce": "<12 bytes, base64>",
  "ct": "<AES-256-GCM ciphertext of the plaintext below, base64>"
}
```

Plaintext (JSON):

```json
{ "key": "VOIC-XXXX-XXXX-XXXX-XXXX", "token": "<signed license token>" }
```

Encryption: `shared = X25519(eph_priv, app_pub)`, `aes_key = HKDF-SHA256(shared, info="voicin-reply-v1")`,
then AES-256-GCM with the nonce above. The app derives the same key with `X25519(app_priv, eph_pub)`.

Error replies use the same envelope, with plaintext `{ "error": "<code>" }`. Codes:
`bad_request`, `stale`, `tx_not_confirmed`, `tx_not_usdt_payment`, `tx_already_used`, `machine_mismatch`, `sold_out`.

## Issuer logic (per request)

1. Parse and validate the request (rules above).
2. Verify the payment with the existing `verifyPayment` in `lib/chain.js` (USDT `Transfer` to the treasury, at least `PRICE_USDT`, successful receipt).
3. Look up `voicin:claim:<tx>` in Redis:
   - Not found: pick an unused key from `LICENSE_KEYS` with an atomic `SET used:<key> <tx> NX`, then store `claim:<tx> = {key, machine}`.
     If no key is free, reply `sold_out`.
   - Found with the same `machine`: reuse the stored key. This handles a retry after a lost reply.
   - Found with a different `machine`: reply `tx_already_used`.
4. Bind the key to the machine: set `lic:<key> = {tx, network, at, machine}` (same format as the current server).
5. Sign the token with `lib/sign.js`: payload `{v:1, kh: sha256(key), mh: machine, iat}`.
6. Encrypt the reply to `pubkey` and publish it to `/voicin/1/res/<reqId>`.

Idempotence rule: the same `tx` and the same `machine` always produce the same key. A different machine never gets a key for a used `tx`.

## App logic

1. On first run, create the X25519 key pair and store the private key locally.
2. Show the machine hash to the user (optional, for support).
3. When the user enters a payment tx hash:
   - Generate `reqId` and subscribe to `/voicin/1/res/<reqId>` (filter or store).
   - Publish the request with lightpush.
   - Wait up to 10 minutes, polling the reply topic every few seconds.
4. Decrypt the reply. Verify the token offline (existing `verify_token`), then save key, machine and token in `license.json`.
5. After that, never contact Waku or any server for licensing.

If the app closes before a reply arrives, the user re-submits the same tx hash and gets the same key back (idempotence rule).

## Transport: nwaku REST (verify endpoint names against the node version you use)

- Publish: `POST /lightpush/v3/message` with `{ "pubsubTopic": "...", "message": { "payload": "<base64>", "contentTopic": "...", "timestamp": <ns> } }`
- Subscribe (app): `POST /filter/v2/subscriptions` then `GET /filter/v2/messages/<contentTopic>`
- Poll history (issuer, or app as a fallback): `GET /store/v3/messages?contentTopic=...&pubsubTopic=...`

## Storage and retention

All keys use the project prefix `voicin:` because the Redis/Valkey database is shared by several projects.


- Waku Store nodes keep messages for a configurable period, 30 days by default. This is not guaranteed by the network.
- Nothing essential depends on it: the issuer's state lives in Redis, and the app keeps its own token.
  Store is only used as a fallback to fetch a reply the app missed.

## Triggering the issuer

- Vercel Cron on the Hobby plan runs at most once a day, which is too slow for this flow.
  Use an external scheduler (for example GitHub Actions every 5 minutes, or cron-job.org) to call `POST /api/issue`.
- `/api/issue` drains the request topic since the last cursor (kept in Redis), handles each request, and advances the cursor.
- Keep the cursor so a request is never handled twice. The `voicin:claim:<tx>` record also protects against duplicates.

## Threats and limits

- **Spam requests:** anyone can publish to the request topic. Each request costs one RPC call, so limit by `machine` and by source where possible.
- **Replay:** `ts` plus the idempotence rule. A replayed request for a used tx on another machine is refused.
- **Eavesdropping:** replies are encrypted to the app's key. Requests reveal only the tx hash and machine hash, both already public or hashed.
- **Key theft from the app:** the app holds no secret that can issue licenses. Stolen tokens still work only on that machine.
- **Availability:** if the issuer is down, activation waits. Activated apps keep working offline.
- **Revocation:** not supported. Once issued, a token is valid for that machine.
