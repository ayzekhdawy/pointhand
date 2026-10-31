//! Window management (minimize, maximize, restore, move, resize, close) and
//! the integrity check that refuses input Windows would drop.
//!
//! Every mutation here goes through `guard_input`: the allow-list, the idle
//! wait, and Escape apply exactly as they do to clicks and keys.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, IsWindow, IsZoomed, PostMessageW, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE,
    WM_CLOSE,
};

use super::{describe, executable_name, note_action, virtual_screen, wants_background, Session};

/// Session-critical processes. Closing one would break the desktop, so
/// `close_window` refuses them even when the allow-list is empty.
const PROTECTED: &[&str] = &[
    "explorer.exe",
    "csrss.exe",
    "winlogon.exe",
    "wininit.exe",
    "dwm.exe",
    "lsass.exe",
    "services.exe",
    "smss.exe",
    "svchost.exe",
    "sihost.exe",
    "fontdrvhost.exe",
    "logonui.exe",
    "ctfmon.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "searchhost.exe",
    "textinputhost.exe",
    "lockapp.exe",
];
const MIN_WINDOW_SIDE: i32 = 100;
const CLOSE_WAIT_MS: u64 = 2000;

impl Session {
    pub fn set_window_state(&mut self, params: &Value) -> Result<Value, String> {
        let window_id = window_param(params)?;
        let state = params
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let background = wants_background(params);
        // Background mode minimizes and restores without taking activation.
        // Windows has no inactive maximize, so maximize still activates.
        let command = match (state.as_str(), background) {
            ("minimize" | "minimized", false) => SW_MINIMIZE,
            ("minimize" | "minimized", true) => SW_SHOWMINNOACTIVE,
            ("maximize" | "maximized", _) => SW_MAXIMIZE,
            ("restore" | "normal", false) => SW_RESTORE,
            ("restore" | "normal", true) => SW_SHOWNOACTIVATE,
            _ => return Err("state must be minimize, maximize, or restore".into()),
        };
        self.guard_mutation(window_id, background)?;
        crate::check_running()?;
        let handle = hwnd(window_id);
        unsafe {
            let _ = ShowWindow(handle, command);
        }
        note_action();
        crate::overlay::touch();
        Ok(json!({"window": self.get_window(window_id)?, "state": show_state(handle)}))
    }

    pub fn move_window(&mut self, params: &Value) -> Result<Value, String> {
        let window_id = window_param(params)?;
        let info = describe(window_id as isize)
            .ok_or_else(|| format!("window {window_id} is not open"))?;
        let x = optional_int(params, "x")?.unwrap_or(info.left);
        let y = optional_int(params, "y")?.unwrap_or(info.top);
        let width = optional_int(params, "width")?.unwrap_or(info.right - info.left);
        let height = optional_int(params, "height")?.unwrap_or(info.bottom - info.top);
        check_placement(x, y, width, height, virtual_screen())?;
        let background = wants_background(params);
        self.guard_mutation(window_id, background)?;
        crate::check_running()?;
        let handle = hwnd(window_id);
        unsafe {
            // A maximized or minimized window ignores the new rectangle.
            if IsZoomed(handle).as_bool() || IsIconic(handle).as_bool() {
                let restore = if background {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_RESTORE
                };
                let _ = ShowWindow(handle, restore);
            }
            SetWindowPos(
                handle,
                None,
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .map_err(|err| format!("could not move window {window_id}: {err}"))?;
        }
        note_action();
        crate::overlay::touch();
        Ok(json!({"window": self.get_window(window_id)?}))
    }

    fn guard_mutation(&self, window_id: i64, background: bool) -> Result<(), String> {
        if background {
            self.guard_background(window_id)
        } else {
            self.guard_input(window_id)
        }
    }

    /// Ask the window to close, as its title-bar X would. The app may show a
    /// save prompt; the result says whether the window is gone.
    pub fn close_window(&mut self, params: &Value) -> Result<Value, String> {
        let window_id = window_param(params)?;
        let info = describe(window_id as isize)
            .ok_or_else(|| format!("window {window_id} is not open"))?;
        if is_protected(&info.app) {
            return Err(format!(
                "{} is part of the Windows session; PointHand does not close it",
                executable_name(&info.app)
            ));
        }
        self.guard_input(window_id)?;
        crate::check_running()?;
        let handle = hwnd(window_id);
        unsafe {
            PostMessageW(Some(handle), WM_CLOSE, WPARAM(0), LPARAM(0))
                .map_err(|err| format!("could not ask window {window_id} to close: {err}"))?;
        }
        note_action();
        let deadline = Instant::now() + Duration::from_millis(CLOSE_WAIT_MS);
        let mut closed = false;
        while Instant::now() < deadline {
            if !unsafe { IsWindow(Some(handle)) }.as_bool() {
                closed = true;
                break;
            }
            crate::cancellable_wait(Duration::from_millis(100))?;
        }
        Ok(json!({
            "close_requested": window_id,
            "closed": closed,
            "note": if closed { "" } else { "the window is still open; it may be asking to save" },
        }))
    }
}

/// Refuse input to a window whose process runs at a higher integrity level
/// than PointHand. Windows drops that input silently, so nothing would happen.
/// Unknown levels keep the old behaviour and let the input through.
pub(super) fn ensure_reachable(window: isize) -> Result<(), String> {
    let Some(info) = describe(window) else {
        return Ok(());
    };
    let (Some(own), Some(target)) = (own_integrity(), process_integrity(info.pid)) else {
        return Ok(());
    };
    if blocked_by_integrity(own, target) {
        return Err(format!(
            "elevated_target: {} runs as administrator and PointHand does not, so Windows would drop the input. No input was sent. Restart that app without elevation or run PointHand elevated.",
            executable_name(&info.app)
        ));
    }
    Ok(())
}

fn blocked_by_integrity(own: u32, target: u32) -> bool {
    target > own
}

fn own_integrity() -> Option<u32> {
    static LEVEL: OnceLock<Option<u32>> = OnceLock::new();
    *LEVEL.get_or_init(|| unsafe { token_integrity(GetCurrentProcess()) })
}

fn process_integrity(pid: u32) -> Option<u32> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let level = token_integrity(process);
        let _ = CloseHandle(process);
        level
    }
}

unsafe fn token_integrity(process: HANDLE) -> Option<u32> {
    let mut token = HANDLE::default();
    OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
    let mut size = 0u32;
    let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut size);
    if size == 0 {
        let _ = CloseHandle(token);
        return None;
    }
    // u64 storage keeps the SID pointer inside the buffer aligned.
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let read = GetTokenInformation(
        token,
        TokenIntegrityLevel,
        Some(buffer.as_mut_ptr().cast()),
        size,
        &mut size,
    );
    let _ = CloseHandle(token);
    read.ok()?;
    let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
    let sid = label.Label.Sid;
    if sid.0.is_null() {
        return None;
    }
    let count = *GetSidSubAuthorityCount(sid);
    if count == 0 {
        return None;
    }
    Some(*GetSidSubAuthority(sid, u32::from(count) - 1))
}

fn is_protected(app: &str) -> bool {
    let name = executable_name(app).to_ascii_lowercase();
    PROTECTED.contains(&name.as_str())
}

/// The new rectangle must be at least 100 px on each side, no larger than
/// twice the virtual desktop, and must overlap the desktop so it stays reachable.
fn check_placement(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    desktop: (i32, i32, i32, i32),
) -> Result<(), String> {
    let (left, top, desk_width, desk_height) = desktop;
    if width < MIN_WINDOW_SIDE || height < MIN_WINDOW_SIDE {
        return Err(format!(
            "width and height must be at least {MIN_WINDOW_SIDE} pixels"
        ));
    }
    if width > desk_width.saturating_mul(2) || height > desk_height.saturating_mul(2) {
        return Err("window would be larger than twice the desktop".into());
    }
    let right = x.checked_add(width).ok_or("x out of range")?;
    let bottom = y.checked_add(height).ok_or("y out of range")?;
    let overlaps = right > left
        && bottom > top
        && x < left.saturating_add(desk_width)
        && y < top.saturating_add(desk_height);
    if !overlaps {
        return Err("window would leave the visible desktop".into());
    }
    Ok(())
}

fn show_state(handle: HWND) -> &'static str {
    unsafe {
        if IsIconic(handle).as_bool() {
            "minimized"
        } else if IsZoomed(handle).as_bool() {
            "maximized"
        } else {
            "normal"
        }
    }
}

pub(super) fn window_param(params: &Value) -> Result<i64, String> {
    params
        .get("window_id")
        .and_then(|v| v.as_i64())
        .filter(|id| *id > 0)
        .ok_or_else(|| "missing window_id".into())
}

fn optional_int(params: &Value, key: &str) -> Result<Option<i32>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| format!("{key} must be an integer")),
    }
}

fn hwnd(window_id: i64) -> HWND {
    HWND(window_id as isize as *mut std::ffi::c_void)
}

#[cfg(test)]
mod tests {
    use super::{blocked_by_integrity, check_placement, is_protected};

    #[test]
    fn session_processes_are_protected_by_exact_name() {
        assert!(is_protected("C:\\Windows\\explorer.exe"));
        assert!(is_protected("C:\\Windows\\System32\\DWM.EXE"));
        assert!(!is_protected("C:\\Tools\\my-explorer.exe"));
        assert!(!is_protected("C:\\Windows\\System32\\notepad.exe"));
    }

    #[test]
    fn only_a_higher_integrity_target_blocks_input() {
        // Medium is 0x2000, High 0x3000, System 0x4000.
        assert!(blocked_by_integrity(0x2000, 0x3000));
        assert!(!blocked_by_integrity(0x3000, 0x3000));
        assert!(!blocked_by_integrity(0x3000, 0x2000));
    }

    #[test]
    fn placement_stays_on_the_desktop() {
        let desktop = (-1920, 0, 3840, 1080);
        assert!(check_placement(100, 100, 800, 600, desktop).is_ok());
        assert!(check_placement(-1800, 50, 800, 600, desktop).is_ok());
        assert!(check_placement(100, 100, 50, 600, desktop).is_err());
        assert!(check_placement(5000, 100, 800, 600, desktop).is_err());
        assert!(check_placement(100, -900, 800, 600, desktop).is_err());
        assert!(check_placement(0, 0, 9000, 600, desktop).is_err());
        assert!(check_placement(i32::MAX - 10, 0, 800, 600, desktop).is_err());
    }
}
