//! Physical-input activity only: no key text, button history, or coordinates are logged.
use serde_json::{json, Value};
use std::cell::Cell;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::StationsAndDesktops::*;
use windows::Win32::System::SystemInformation::{GetTickCount, GetTickCount64};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

/// A whole batch owns the desktop input lane. Different MCP processes cannot
/// interleave mouse-down/key-down sequences. The OS releases abandoned leases.
pub struct InputLock(HANDLE);
impl InputLock {
    pub fn for_request(method: &str, params: &Value) -> Result<Option<Self>, String> {
        let input = match method {
            "list_apps" | "list_windows" | "get_window" | "get_system_info" | "screenshot"
            | "clipboard_read" | "overlay_status" | "guard_status" | "ping" | "close" => false,
            "get_window_state" => params
                .get("include_screenshot")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            _ => true,
        };
        if !input {
            return Ok(None);
        }
        unsafe {
            let handle = CreateMutexW(None, false, windows::core::w!("Local\\PointHand.Input"))
                .map_err(|e| e.to_string())?;
            let result = WaitForSingleObject(handle, 5000);
            if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Err(
                    "input_busy: another PointHand process owns the desktop input lane".into(),
                );
            }
            Ok(Some(Self(handle)))
        }
    }
}
impl Drop for InputLock {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

static READY: AtomicBool = AtomicBool::new(false);
static LAST_PHYSICAL: AtomicU64 = AtomicU64::new(0);
static EPOCH: AtomicU64 = AtomicU64::new(0);
thread_local! {static ACTION_EPOCH:Cell<Option<u64>>=const {Cell::new(None)};}
pub fn status() -> Value {
    json!({"ready":READY.load(Ordering::Acquire),"physical_idle_ms":idle_ms(),"physical_events":EPOCH.load(Ordering::Relaxed),"desktop_available":desktop_available().is_ok(),"logs_input_content":false})
}
pub fn idle_ms() -> u64 {
    unsafe { GetTickCount64().saturating_sub(LAST_PHYSICAL.load(Ordering::Relaxed)) }
}
pub fn begin_action() {
    ACTION_EPOCH.with(|slot| slot.set(Some(EPOCH.load(Ordering::Acquire))));
}
pub fn end_action() {
    ACTION_EPOCH.with(|slot| slot.set(None));
}
pub fn check_action() -> Result<(), String> {
    ACTION_EPOCH.with(|slot|{
    if slot.get().is_some_and(|epoch|epoch!=EPOCH.load(Ordering::Acquire)){
        Err("user_active: physical input interrupted this action; injected buttons are being released".into())
    }else{Ok(())}
})
}
pub fn prepare() -> Result<(), String> {
    if !READY.load(Ordering::Acquire) {
        return Err("input_guard_unavailable: physical-input monitoring is not ready".into());
    }
    desktop_available()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while idle_ms() < 400 {
        crate::check_running()?;
        if Instant::now() >= deadline {
            return Err("user_active: the keyboard or mouse is in use".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
pub fn desktop_available() -> Result<(), String> {
    unsafe {
        let desktop = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS)
            .map_err(|_| "desktop_unavailable: unlock the desktop or close the secure prompt")?;
        let mut name = [0u16; 256];
        let result = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            (name.len() * 2) as u32,
            None,
        );
        let _ = CloseDesktop(desktop);
        result.map_err(|_| "desktop_unavailable: cannot inspect the input desktop")?;
        let end = name.iter().position(|v| *v == 0).unwrap_or(name.len());
        if !String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case("default") {
            return Err(
                "desktop_unavailable: input on a secure or non-default desktop is refused".into(),
            );
        }
        Ok(())
    }
}

fn physical(flags: u32, keyboard: bool) -> bool {
    flags
        & if keyboard {
            LLKHF_INJECTED.0
        } else {
            LLMHF_INJECTED
        }
        == 0
}
fn record() {
    unsafe {
        LAST_PHYSICAL.store(GetTickCount64(), Ordering::Relaxed);
    }
    EPOCH.fetch_add(1, Ordering::Release);
}
unsafe extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && lp.0 != 0 {
        let data = &*(lp.0 as *const MSLLHOOKSTRUCT);
        if physical(data.flags, false) {
            record();
        }
    }
    CallNextHookEx(None, code, wp, lp)
}
unsafe extern "system" fn key_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && lp.0 != 0 {
        let data = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        if physical(data.flags.0, true) {
            record();
            if data.vkCode == VK_ESCAPE.0 as u32
                && [WM_KEYDOWN as usize, WM_SYSKEYDOWN as usize].contains(&wp.0)
            {
                crate::STOPPED.store(true, Ordering::Release);
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}
struct Hook(HHOOK);
impl Drop for Hook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
        }
    }
}
pub struct InputGuard {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl InputGuard {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                unsafe {
                    let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.to_string())?.0);
                    let _mouse = Hook(
                        SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(instance), 0)
                            .map_err(|e| e.to_string())?,
                    );
                    let _keyboard = Hook(
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), Some(instance), 0)
                            .map_err(|e| e.to_string())?,
                    );
                    let mut last = LASTINPUTINFO {
                        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                        dwTime: 0,
                    };
                    let _ = GetLastInputInfo(&mut last);
                    LAST_PHYSICAL.store(
                        GetTickCount64()
                            .saturating_sub(GetTickCount().wrapping_sub(last.dwTime) as u64),
                        Ordering::Relaxed,
                    );
                    READY.store(true, Ordering::Release);
                    let _ = tx.send(());
                    while !stopped.load(Ordering::Relaxed) {
                        let mut msg = MSG::default();
                        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                        thread::sleep(Duration::from_millis(4));
                    }
                    Ok(())
                }
            })();
            if let Err(error) = result {
                eprintln!("PointHand physical-input guard unavailable: {error}");
            }
            READY.store(false, Ordering::Release);
        });
        let _ = rx.recv_timeout(Duration::from_secs(2));
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for InputGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_events_do_not_look_like_human_input() {
        assert!(!physical(LLMHF_INJECTED, false));
        assert!(!physical(LLKHF_INJECTED.0, true));
        assert!(physical(0, false));
        assert!(physical(0, true));
    }
    #[test]
    fn modifier_flags_alone_are_still_physical() {
        assert!(physical(LLKHF_ALTDOWN.0, true));
        assert!(!physical(LLKHF_ALTDOWN.0 | LLKHF_INJECTED.0, true));
    }
}
