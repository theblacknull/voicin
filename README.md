# Voicin

Website, license service and download page for **Voicin**, a voice-typing app for Windows.

Deployed on **Vercel** from this repository:

- `public/`: landing page and downloads (`public/downloads/Voicin-Setup.exe`)
- `api/` + `lib/`: license API (Vercel Functions) with Upstash Redis
- `app/`: source code of the Windows app (Tauri v2)

## API

| Endpoint | Description |
| --- | --- |
| `GET /api/info` | Price, receiving wallet, supported networks |
| `POST /api/claim` | `{network: "bsc" \| "eth", tx_hash}`: verifies the USDT payment on-chain and returns `{key}` |
| `POST /api/activate` | `{key, machine}`: binds a key to its first device; other devices are refused |

## Deploy on Vercel

1. Import this GitHub repository in Vercel (Framework preset: **Other**, root directory: `/`).
2. **Storage**: add **Upstash Redis**. It creates `UPSTASH_REDIS_REST_URL` and `UPSTASH_REDIS_REST_TOKEN`.
3. **Settings → Environment Variables**: add `TREASURY_EVM`, `PRICE_USDT=10` and `LICENSE_KEYS` (comma-separated, from your local `.env`).
4. Deploy. The app reads the license server URL from `VOICIN_LICENSE_URL`.

## Security notes

- `.env` holds the key list. It is git-ignored; enter its values only in Vercel.
- To add keys, update `LICENSE_KEYS` in Vercel and redeploy.
- Build the Windows app from `app/` (see `app/README.md`), then copy the installer to `public/downloads/Voicin-Setup.exe`.
