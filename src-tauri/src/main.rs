// Voicin: nhập giọng nói vào vị trí con trỏ ở bất kỳ app nào.
// - Nhấn và thả Ctrl để BẬT/TẮT nghe. Thả phím vẫn tiếp tục nghe.
// - Âm thanh micro được cắt thành từng câu (theo khoảng lặng), gửi lên Google STT,
//   rồi chữ được gõ vào cửa sổ đang focus. Cửa sổ Tauri chỉ hiển thị trạng thái.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod asr;
mod audio;
mod keys;
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
// Ngôn ngữ đang dùng: một mã (ví dụ "vi-VN") hoặc nhiều mã khi chọn "Tự động"
static LANGS: RwLock<Vec<String>> = RwLock::new(Vec::new());
const AUTO_LANGS: &str = "vi-VN,en-US,zh-CN,ja-JP,ko-KR,fr-FR,de-DE,es-ES,ru-RU";

/// Giao diện gọi khi người dùng chọn ngôn ngữ. "auto" = tự nhận diện trong danh sách.
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

/// Gửi sự kiện tới giao diện (nếu cửa sổ đã sẵn sàng)
fn emit<S: serde::Serialize + Clone>(event: &str, payload: S) {
    if let Some(app) = APP.get() {
        let _ = app.emit(event, payload);
    }
}

const FRAME: usize = 480; // 30 ms ở 16 kHz
const MIN_UTTERANCE: usize = 6400; // 0.4 giây: bỏ qua tiếng click
const MAX_UTTERANCE: usize = 16000 * 12; // tối đa 12 giây mỗi câu
const END_SILENCE_FRAMES: usize = 20; // 600 ms im lặng thì kết thúc câu
const DEFAULT_LANGS: &str = "vi-VN";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--keys") {
        // Chế độ chẩn đoán: in mã phím khi nhấn, để biết phím Fn/phím bạn chọn gửi mã nào
        println!("Nhấn phím cần kiểm tra (Ctrl+C để thoát):");
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
        eprintln!("[Voicin] không mở được micro: {e}");
        return;
    }

    thread::spawn(move || segment_loop(chunk_rx, utt_tx));
    thread::spawn(move || recognize_loop(utt_rx));

    keys::set_trigger(trigger);
    keys::start(toggle);

    println!("[Voicin] Sẵn sàng. Nhấn phím kích hoạt (mã 0x{trigger:02X}) để bật/tắt nghe.");
    // Stream micro phải sống đến khi thoát app (giữ trong biến `stream`)
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![set_language])
        .setup(|app| {
            let _ = APP.set(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("không chạy được Voicin");
    drop(stream);
}

/// Đảo trạng thái nghe mỗi lần nhấn phím (không tính lúc giữ phím lặp)
fn toggle() {
    let was_on = LISTENING.fetch_xor(true, Ordering::SeqCst);
    emit("listening", !was_on);
    if was_on {
        println!("[Voicin] TẮT nghe");
    } else {
        println!("[Voicin] BẬT nghe — nói vào micro, chữ sẽ hiện ở con trỏ");
    }
}

fn rms(frame: &[i16]) -> f32 {
    let sum: f32 = frame.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum();
    (sum / frame.len() as f32).sqrt()
}

/// Cắt dòng âm thanh thành từng câu dựa trên mức năng lượng (VAD đơn giản, thích nghi theo tiếng ồn nền)
fn segment_loop(rx: Receiver<Vec<i16>>, tx: Sender<Vec<i16>>) {
    let mut pending: Vec<i16> = Vec::new();
    let mut pre: VecDeque<Vec<i16>> = VecDeque::new(); // 300 ms trước khi phát hiện tiếng nói
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
                // Vừa tắt nghe: gửi nốt câu đang nói dở rồi reset
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

/// Nhận dạng từng câu rồi gõ chữ vào con trỏ
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
                // Các câu liên tiếp trong vài giây được ngăn cách bằng dấu cách
                let recent = last_typed.map_or(false, |at| at.elapsed() < Duration::from_secs(20));
                let out = if recent { format!(" {t}") } else { t.to_string() };
                typer::type_text(&out);
                last_typed = Some(Instant::now());
                println!("[Voicin] {t}");
                emit("text", t.to_string());
            }
            Err(e) => eprintln!("[Voicin] lỗi nhận dạng: {e}"),
        }
    }
}
