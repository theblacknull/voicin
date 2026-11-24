# Voicin app (Windows)

Voice typing app built with Tauri v2 and Rust. Press and release Ctrl to start or stop listening; speech is typed at the cursor in any app.

## Build

Requirements: Node.js, Rust (MSVC), Visual Studio Build Tools (C++).

```bash
npm install
npx tauri build
```

The installer is written to `src-tauri/target/release/bundle/nsis/`. Copy it to `../public/downloads/Voicin-Setup.exe` so the website can serve it.

## Configuration (environment variables)

| Variable | Default | Meaning |
| --- | --- | --- |
| `VOICIN_LICENSE_URL` | `http://127.0.0.1:8787` | Base URL of the license API (your Vercel deployment) |
| `VOICIN_KEY` | `0x11` (Ctrl) | Trigger key code (hex) |
| `VOICIN_LANGS` | `vi-VN` | Candidate recognition languages, comma-separated |

## Structure

- `src-tauri/src/main.rs`: app state, commands, speech pipeline
- `src-tauri/src/audio.rs`: microphone capture (cpal/WASAPI), resampling to 16 kHz
- `src-tauri/src/asr.rs`: Google Speech requests and language selection
- `src-tauri/src/keys.rs`: low-level keyboard hook for the trigger key
- `src-tauri/src/typer.rs`: types text with `SendInput` (Unicode)
- `src-tauri/src/license.rs`: 4-hour trial, key activation, purchase claim
- `ui/`: the mini window

## Notes

- Uses Google's unofficial speech endpoint. It may change or stop at any time.
- Requires an internet connection.
