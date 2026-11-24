// Bản quyền: dùng thử 4 giờ, sau đó cần license key (mua trọn đời bằng USDT).
// Key và ví nhận tiền nằm trên license server (license-server/), app chỉ gọi API.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const TRIAL_SECS: u64 = 4 * 60 * 60;
const DEFAULT_SERVER: &str = "http://127.0.0.1:8787";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    first_run: u64,
    key: Option<String>,
    last_ok: u64,
}

#[derive(Serialize, Clone)]
pub struct Status {
    /// "trial" | "trial_expired" | "licensed"
    pub state: String,
    pub remaining_secs: u64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn server() -> String {
    std::env::var("VOICIN_LICENSE_URL").unwrap_or_else(|_| DEFAULT_SERVER.into())
}

fn file() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    base.join("Voicin").join("license.json")
}

fn load() -> Saved {
    std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(s: &Saved) {
    let p = file();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(p, text);
    }
}

/// Đọc trạng thái; lần đầu chạy thì ghi mốc bắt đầu dùng thử
fn load_or_init() -> Saved {
    let mut s = load();
    if s.first_run == 0 {
        s.first_run = now();
        save(&s);
    }
    s
}

pub fn status() -> Status {
    let s = load_or_init();
    if s.key.is_some() {
        return Status { state: "licensed".into(), remaining_secs: 0 };
    }
    let used = now().saturating_sub(s.first_run);
    if used < TRIAL_SECS {
        Status { state: "trial".into(), remaining_secs: TRIAL_SECS - used }
    } else {
        Status { state: "trial_expired".into(), remaining_secs: 0 }
    }
}

pub fn can_use() -> bool {
    status().state != "trial_expired"
}

/// Mã định danh máy (MachineGuid của Windows), dùng để gắn key với một máy
fn machine_id() -> String {
    let out = std::process::Command::new("reg")
        .args(["query", "HKLM\\SOFTWARE\\Microsoft\\Cryptography", "/v", "MachineGuid"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        if let Some(guid) = text.lines().find(|l| l.contains("MachineGuid")).and_then(|l| l.split_whitespace().last()) {
            return guid.to_string();
        }
    }
    "unknown-device".into()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build()
}

/// Trả về (mã HTTP, JSON). Lỗi mạng trả về Err.
fn post(path: &str, body: Value) -> Result<(u16, Value), String> {
    let url = format!("{}{}", server(), path);
    match agent().post(&url).send_json(body) {
        Ok(r) => {
            let code = r.status();
            Ok((code, r.into_json().unwrap_or(json!({}))))
        }
        Err(ureq::Error::Status(code, r)) => Ok((code, r.into_json().unwrap_or(json!({})))),
        Err(e) => Err(e.to_string()),
    }
}

fn err_text(v: &Value) -> String {
    v.get("error").and_then(|e| e.as_str()).unwrap_or("request failed").to_string()
}

/// Thông tin mua hàng từ server: giá, ví nhận, mạng hỗ trợ
pub fn info() -> Result<Value, String> {
    let url = format!("{}/api/info", server());
    agent().get(&url).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())
}

/// Kích hoạt bằng key người dùng nhập
pub fn activate(key: &str) -> Result<(), String> {
    let (code, v) = post("/api/activate", json!({ "key": key, "machine": machine_id() }))?;
    if code == 200 {
        let mut s = load_or_init();
        s.key = Some(key.trim().to_uppercase());
        s.last_ok = now();
        save(&s);
        Ok(())
    } else {
        Err(err_text(&v))
    }
}

/// Mua: gửi hash giao dịch USDT, server xác minh trên blockchain rồi trả về key
pub fn claim(network: &str, tx_hash: &str) -> Result<(), String> {
    let (code, v) = post("/api/claim", json!({ "network": network, "tx_hash": tx_hash.trim() }))?;
    if code != 200 {
        return Err(err_text(&v));
    }
    let key = v.get("key").and_then(|k| k.as_str()).ok_or("server did not return a key")?.to_string();
    activate(&key)
}

/// Khi mở app: kiểm tra lại key với server (chống dùng chung). Mất mạng thì vẫn cho dùng.
pub fn validate_on_start() {
    let s = load();
    let Some(key) = s.key.clone() else { return };
    match post("/api/activate", json!({ "key": key, "machine": machine_id() })) {
        Ok((200, _)) => {
            let mut s = s;
            s.last_ok = now();
            save(&s);
        }
        Ok((403, _)) | Ok((404, _)) => {
            // Key bị thu hồi hoặc đang dùng trên máy khác: đưa về trạng thái chưa kích hoạt
            let mut s = s;
            s.key = None;
            save(&s);
        }
        _ => {} // lỗi mạng: giữ nguyên
    }
}
