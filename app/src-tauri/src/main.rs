// Voicin: type by voice at the cursor position in any app.
// - Press and release Ctrl to toggle listening. Listening continues after the key is released.
// - Microphone audio is split into utterances (on silence), sent to Google Speech,
//   and the recognized text is typed into the focused window. The Tauri window only shows status.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod asr;
mod audio;
mod keys;
mod license;
mod typer;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub static LISTENING: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();
// Active recognition language: one code (e.g. "vi-VN") or several when "auto" is selected
static LANGS: RwLock<Vec<String>> = RwLock::new(Vec::new());
const AUTO_LANGS: &str = "vi-VN,en-US,zh-CN,ja-JP,ko-KR,fr-FR,de-DE,es-ES,ru-RU";

/// Called by the UI when the user picks a language. "auto" = detect among the candidates.
#[tauri::command]
fn set_language(code: String) {
    let list: Vec<String> = if code == "auto" {
        AUTO_LANGS.split(',').map(String::from).collect()
    } else {
        vec![code]
    };
    if let Ok(mut g) = LANGS.write() {
        *g = list;
    }
}

/// Sends an event to the UI (if the window is ready)
fn emit<S: serde::Serialize + Clone>(event: &str, payload: S) {
    if let Some(app) = APP.get() {
        let _ = app.emit(event, payload);
    }
}

const FRAME: usize = 480; // 30 ms at 16 kHz
const MIN_UTTERANCE: usize = 6400; // 0.4 s: ignore clicks
const MAX_UTTERANCE: usize = 16000 * 12; // at most 12 s per utterance
const END_SILENCE_FRAMES: usize = 20; // 600 ms of silence ends an utterance
const DEFAULT_LANGS: &str = "vi-VN";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--keys") {
        // Diagnostic mode: print key codes as they are pressed
        println!("Press the key to check (Ctrl+C to quit):");
        keys::set_debug(true);
        keys::start(|| {});
        loop {
            thread::park();
        }
    }

    let trigger = std::env::var("VOICIN_KEY")
        .ok()
        .and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x11); // Ctrl (scan code 29)
    let langs: Vec<String> = std::env::var("VOICIN_LANGS")
        .unwrap_or_else(|_| DEFAULT_LANGS.to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if let Ok(mut g) = LANGS.write() {
        *g = langs;
    }

    let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<i16>>();
    let (utt_tx, utt_rx) = mpsc::channel::<Vec<i16>>();

    let stream = match audio::start(chunk_tx) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Voicin] {e}");
            return;
        }
    };
    if let Err(e) = <cpal::Stream as cpal::traits::StreamTrait>::play(&stream) {
        eprintln!("[Voicin] could not open the microphone: {e}");
        return;
    }

    thread::spawn(move || segment_loop(chunk_rx, utt_tx));
    thread::spawn(move || recognize_loop(utt_rx));

    keys::set_trigger(trigger);
    keys::start(toggle);

    println!("[Voicin] Ready. Press the trigger key (0x{trigger:02X}) to toggle listening.");
    // The microphone stream must stay alive until the app exits (kept in `stream`)
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            set_language,
            license_status,
            license_info,
            license_activate,
            license_claim
        ])
        .setup(|app| {
            let _ = APP.set(app.handle().clone());
            thread::spawn(license::validate_on_start);
            license_ticker();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Voicin failed to start");
    drop(stream);
}

/// UI command: license status
#[tauri::command]
fn license_status() -> license::Status {
    license::status()
}

#[tauri::command]
async fn license_info() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(license::info).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn license_activate(key: String) -> Result<license::Status, String> {
    tauri::async_runtime::spawn_blocking(move || license::activate(&key)).await.map_err(|e| e.to_string())??;
    Ok(license::status())
}

#[tauri::command]
async fn license_claim(network: String, tx_hash: String) -> Result<license::Status, String> {
    tauri::async_runtime::spawn_blocking(move || license::claim(&network, &tx_hash))
        .await
        .map_err(|e| e.to_string())??;
    Ok(license::status())
}

/// Every second: send the license status to the UI; stop listening once the trial has expired
fn license_ticker() {
    thread::spawn(|| loop {
        let st = license::status();
        if st.state == "trial_expired" && LISTENING.swap(false, Ordering::SeqCst) {
            emit("listening", false);
        }
        emit("license", st);
        thread::sleep(Duration::from_secs(1));
    });
}

/// Flips listening state on each trigger press (ignores held-key repeats)
fn toggle() {
    if !LISTENING.load(Ordering::SeqCst) && !license::can_use() {
        emit("locked", ());
        return;
    }
    let was_on = LISTENING.fetch_xor(true, Ordering::SeqCst);
    emit("listening", !was_on);
    if was_on {
        println!("[Voicin] listening OFF");
    } else {
        println!("[Voicin] listening ON: speak into the microphone; text appears at the cursor");
    }
}

fn rms(frame: &[i16]) -> f32 {
    let sum: f32 = frame.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum();
    (sum / frame.len() as f32).sqrt()
}

/// Splits the audio stream into utterances based on energy (simple VAD that adapts to background noise)
fn segment_loop(rx: Receiver<Vec<i16>>, tx: Sender<Vec<i16>>) {
    let mut pending: Vec<i16> = Vec::new();
    let mut pre: VecDeque<Vec<i16>> = VecDeque::new(); // 300 ms before speech is detected
    let mut buf: Vec<i16> = Vec::new();
    let mut in_speech = false;
    let mut loud_run = 0usize;
    let mut quiet_run = 0usize;
    let mut noise = 0.005f32;

    let flush = |buf: &mut Vec<i16>, tx: &Sender<Vec<i16>>| {
        if buf.len() >= MIN_UTTERANCE {
            let _ = tx.send(std::mem::take(buf));
        } else {
            buf.clear();
        }
    };

    for chunk in rx {
        pending.extend_from_slice(&chunk);
        while pending.len() >= FRAME {
            let frame: Vec<i16> = pending.drain(..FRAME).collect();
            let listening = LISTENING.load(Ordering::SeqCst);
            emit("level", if listening { (rms(&frame) * 8.0).min(1.0) } else { 0.0 });

            if !listening {
                // Listening just stopped: send the unfinished utterance, then reset
                if in_speech {
                    flush(&mut buf, &tx);
                }
                in_speech = false;
                pre.clear();
                loud_run = 0;
                continue;
            }

            let level = rms(&frame);
            let threshold = (noise * 3.0).max(0.012);
            let loud = level > threshold;

            if !in_speech {
                if !loud {
                    noise = noise * 0.95 + level * 0.05;
                }
                pre.push_back(frame);
                if pre.len() > 10 {
                    pre.pop_front();
                }
                loud_run = if loud { loud_run + 1 } else { 0 };
                if loud_run >= 3 {
                    in_speech = true;
                    buf = pre.drain(..).flatten().collect();
                    quiet_run = 0;
                }
            } else {
                buf.extend_from_slice(&frame);
                quiet_run = if loud { 0 } else { quiet_run + 1 };
                if quiet_run >= END_SILENCE_FRAMES || buf.len() >= MAX_UTTERANCE {
                    flush(&mut buf, &tx);
                    in_speech = false;
                    pre.clear();
                    loud_run = 0;
                }
            }
        }
    }
}

/// Recognizes each utterance and types the text at the cursor
fn recognize_loop(rx: Receiver<Vec<i16>>) {
    let mut last_typed: Option<Instant> = None;
    for pcm in rx {
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let langs = LANGS.read().map(|g| g.clone()).unwrap_or_default();
        match asr::stt_best(&bytes, &langs) {
            Ok(text) => {
                let t = text.trim();
                if t.is_empty() {
                    continue;
                }
                // Consecutive utterances within a few seconds are separated by a space
                let recent = last_typed.map_or(false, |at| at.elapsed() < Duration::from_secs(20));
                let out = if recent { format!(" {t}") } else { t.to_string() };
                typer::type_text(&out);
                last_typed = Some(Instant::now());
                println!("[Voicin] {t}");
                emit("text", t.to_string());
            }
            Err(e) => eprintln!("[Voicin] recognition error: {e}"),
        }
    }
}
