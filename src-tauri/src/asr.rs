// Google Speech (endpoint web không chính thức, cùng cách làm với bản dịch trong ../src-tauri/src/google.rs).
// Mỗi câu được gửi song song cho nhiều ngôn ngữ; chọn kết quả đúng chữ viết và tin cậy nhất.
use serde_json::Value;
use std::time::Duration;

const STT_URL: &str = "https://www.google.com/speech-api/v2/recognize";
// Khóa công khai của chức năng nhận dạng giọng nói Chromium
const STT_KEY: &str = "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

fn agent() -> ureq::Agent {
    let tls = native_tls::TlsConnector::new().expect("native TLS");
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .tls_connector(std::sync::Arc::new(tls))
        .build()
}

/// Nhận dạng một ngôn ngữ. PCM là mono 16 kHz, 16-bit little-endian. Trả về (chữ, độ tin cậy).
pub fn stt(pcm: &[u8], lang: &str) -> Result<(String, f64), String> {
    let url = format!("{STT_URL}?output=json&lang={lang}&key={STT_KEY}&client=chromium");
    let resp = agent()
        .post(&url)
        .set("User-Agent", UA)
        .set("Content-Type", "audio/l16; rate=16000")
        .send_bytes(pcm)
        .map_err(|e| format!("Google STT lỗi: {e}"))?;
    let body = resp.into_string().map_err(|e| e.to_string())?;
    for line in body.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let alt = &v["result"][0]["alternative"][0];
        if let Some(t) = alt["transcript"].as_str() {
            let conf = alt["confidence"].as_f64().unwrap_or(0.5);
            return Ok((t.to_string(), conf));
        }
    }
    Ok((String::new(), 0.0))
}

/// Kết quả có đúng chữ viết của ngôn ngữ không? (độ tin cậy giữa các ngôn ngữ không so sánh được)
fn script_fits(text: &str, lang: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return false;
    }
    let n = letters.len();
    let count = |f: fn(char) -> bool| letters.iter().filter(|c| f(**c)).count();
    let cyr = count(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
    let han = count(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
    let kana = count(|c| ('\u{3040}'..='\u{30FF}').contains(&c));
    let hangul = count(|c| ('\u{AC00}'..='\u{D7AF}').contains(&c) || ('\u{1100}'..='\u{11FF}').contains(&c));
    let thai = count(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c));
    let viet = count(|c| "ăâđêôơưĂÂĐÊÔƠƯ".contains(c) || ('\u{1EA0}'..='\u{1EF9}').contains(&c));
    let major = |x: usize| x * 2 > n;
    let l = lang.to_lowercase();
    if l.starts_with("ru") {
        major(cyr)
    } else if l.starts_with("zh") {
        major(han) && kana == 0
    } else if l.starts_with("ja") {
        kana > 0 || major(han)
    } else if l.starts_with("ko") {
        major(hangul)
    } else if l.starts_with("th") {
        major(thai)
    } else if l.starts_with("vi") {
        viet > 0 && !major(cyr)
    } else {
        // Các ngôn ngữ Latin khác: không có chữ Cyrillic/CJK/Hangul/Thái, không có dấu tiếng Việt
        !major(cyr) && !major(han) && !major(kana) && !major(hangul) && !major(thai) && viet == 0
    }
}

/// Nhận dạng với nhiều ngôn ngữ ứng viên, song song. Ưu tiên kết quả đúng chữ viết và tin cậy nhất.
pub fn stt_best(pcm: &[u8], langs: &[String]) -> Result<String, String> {
    let results: Vec<(String, Result<(String, f64), String>)> = std::thread::scope(|sc| {
        let handles: Vec<_> = langs
            .iter()
            .map(|l| {
                let lang = l.clone();
                (lang.clone(), sc.spawn(move || stt(pcm, &lang)))
            })
            .collect();
        handles
            .into_iter()
            .map(|(l, h)| (l, h.join().unwrap_or_else(|_| Err("lỗi nội bộ".into()))))
            .collect()
    });

    let mut best: Option<(f64, String)> = None;
    let mut fallback: Option<String> = None;
    let mut last_err = None;
    for (lang, r) in results {
        match r {
            Ok((text, conf)) if !text.trim().is_empty() => {
                if fallback.is_none() {
                    fallback = Some(text.clone());
                }
                if script_fits(&text, &lang) && best.as_ref().map_or(true, |(c, _)| conf > *c) {
                    best = Some((conf, text));
                }
            }
            Ok(_) => {}
            Err(e) => last_err = Some(e),
        }
    }
    match (best, fallback, last_err) {
        (Some((_, t)), _, _) => Ok(t),
        (None, Some(t), _) => Ok(t),
        (None, None, Some(e)) => Err(e),
        (None, None, None) => Ok(String::new()),
    }
}
