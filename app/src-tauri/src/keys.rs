// Low-level keyboard hook: catches the trigger key and fires once per press (no repeats while held).
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use std::thread;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

static TRIGGER: AtomicU32 = AtomicU32::new(0x11); // Ctrl by default (scan code 29)
static HELD: AtomicBool = AtomicBool::new(false);
// A different key was pressed while the trigger was held (e.g. Ctrl+C): not a toggle
static DIRTY: AtomicBool = AtomicBool::new(false);
static DEBUG: AtomicBool = AtomicBool::new(false);
type Callback = Box<dyn Fn() + Send + Sync>;
static ON_TRIGGER: OnceLock<Callback> = OnceLock::new();

pub fn set_trigger(vk: u32) {
    TRIGGER.store(vk, Ordering::SeqCst);
}

pub fn set_debug(on: bool) {
    DEBUG.store(on, Ordering::SeqCst);
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        let vk = info.vkCode;

        if DEBUG.load(Ordering::SeqCst) && down {
            println!("vkCode = 0x{vk:02X}  scanCode = {}", info.scanCode);
        }
        let trig = TRIGGER.load(Ordering::SeqCst);
        // Left Ctrl (A2) and right Ctrl (A3) both count as Ctrl
        let is_trigger = vk == trig || (trig == 0x11 && (vk == 0xA2 || vk == 0xA3));
        if is_trigger {
            if down {
                // Ignore auto-repeat while the key is held
                if !HELD.swap(true, Ordering::SeqCst) {
                    DIRTY.store(false, Ordering::SeqCst);
                }
            } else if up && HELD.swap(false, Ordering::SeqCst) {
                // Only a clean tap toggles listening; Ctrl+C and similar combos are ignored
                if !DIRTY.load(Ordering::SeqCst) {
                    if let Some(cb) = ON_TRIGGER.get() {
                        cb();
                    }
                }
            }
        } else if down && HELD.load(Ordering::SeqCst) {
            DIRTY.store(true, Ordering::SeqCst);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Starts the hook on its own thread; `on_trigger` is called on each trigger tap
pub fn start(on_trigger: impl Fn() + Send + Sync + 'static) {
    let _ = ON_TRIGGER.set(Box::new(on_trigger));
    thread::spawn(|| unsafe {
        let hmod = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or(HINSTANCE(std::ptr::null_mut()));
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hmod, 0);
        if hook.is_err() {
            eprintln!("[Voicin] could not install the keyboard hook");
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
    });
}
