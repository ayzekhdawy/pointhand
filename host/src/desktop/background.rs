//! Background mode: work in one window while the user keeps working in another.
//!
//! The shared pointer and keyboard are never touched. Accessibility actions
//! (`set_value`, `perform_secondary_action`) run without moving focus, and
//! pointer and key input is posted as window messages to the target window,
//! even when another window covers it. Because nothing physical is injected,
//! background calls skip the "wait until the user is idle" gate.
//!
//! What still applies: the `POINTHAND_ALLOW` allow-list, the integrity check,
//! the desktop-locked check, and physical Escape, which stops every mutation.
//!
//! Posted messages reach classic Win32 controls reliably. Chromium, Electron
//! and some WinUI surfaces may ignore them, so results report
//! `delivery: "posted"` and the agent should verify with a window screenshot.

use std::time::Duration;

use serde_json::{json, Value};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{
    ChildWindowFromPointEx, GetGUIThreadInfo, GetWindowThreadProcessId, IsIconic, PostMessageW,
    CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, GUITHREADINFO, WM_CHAR, WM_KEYDOWN,
    WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN,
    WM_RBUTTONUP,
};

use super::{densify, describe, int_key, note_action, owner_of, point_in_frame, Session};

const MK_LBUTTON: usize = 0x0001;
const MK_RBUTTON: usize = 0x0002;
const MK_MBUTTON: usize = 0x0010;
const WHEEL_DELTA: i32 = 120;
const MOVE_GAP_MS: u64 = 4;
const MAX_BACKGROUND_TEXT: usize = 20_000;

/// `background: true` on the call, or `POINTHAND_BACKGROUND=1` as the default.
/// An explicit `background: false` always wins.
pub(super) fn wants_background(params: &Value) -> bool {
    match params.get("background").and_then(|v| v.as_bool()) {
        Some(explicit) => explicit,
        None => default_on(std::env::var("POINTHAND_BACKGROUND").ok().as_deref()),
    }
}

fn default_on(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

#[derive(Clone, Copy)]
enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "left" | "l" => Ok(Self::Left),
            "right" | "r" => Ok(Self::Right),
            "middle" | "m" => Ok(Self::Middle),
            _ => Err("unknown mouse button".into()),
        }
    }

    /// Down, up, double-click message and the held-button flag.
    fn messages(self) -> (u32, u32, u32, usize) {
        match self {
            Self::Left => (WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK, MK_LBUTTON),
            Self::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDBLCLK, MK_RBUTTON),
            Self::Middle => (WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MBUTTONDBLCLK, MK_MBUTTON),
        }
    }
}

impl Session {
    /// Allow-list, integrity and Escape, but no idle wait: nothing physical moves.
    pub(super) fn guard_background(&self, window_id: i64) -> Result<(), String> {
        crate::check_running()?;
        crate::guard::desktop_available()?;
        self.ensure_allowed(window_id)?;
        super::windows_ops::ensure_reachable(window_id as isize)?;
        crate::guard::begin_action();
        Ok(())
    }

    /// Screen point plus the top-level window it belongs to. Background input
    /// always needs a window: `element_index`, a window `screenshotId`, or
    /// `window_id` with window-pixel `x`/`y`.
    fn background_point(&self, params: &Value) -> Result<(i64, i32, i32), String> {
        let hinted = params.get("window_id").and_then(|v| v.as_i64());
        let uses_element = params
            .get("element_index")
            .and_then(|v| v.as_i64())
            .is_some_and(|index| index >= 0);
        if !uses_element {
            if let Some(frame) = self.bound_frame(params, hinted)? {
                if frame.0 == 0 {
                    return Err(background_needs_window());
                }
                let point =
                    point_in_frame(int_key(params, &["x"])?, int_key(params, &["y"])?, frame)?;
                return Ok((frame.0, point.0, point.1));
            }
        }
        let (x, y, window) = self.resolve_point(params)?;
        let window = window.ok_or_else(background_needs_window)?;
        Ok((window, x, y))
    }

    pub(super) fn background_click(&mut self, params: &Value) -> Result<Value, String> {
        let (window, x, y) = self.background_point(params)?;
        let button = Button::parse(
            params
                .get("button")
                .or_else(|| params.get("mouse_button"))
                .and_then(|v| v.as_str())
                .unwrap_or("left"),
        )?;
        let count = params
            .get("count")
            .or_else(|| params.get("click_count"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1);
        if !(1..=3).contains(&count) {
            return Err("click count must be 1, 2, or 3".into());
        }
        self.guard_background(window)?;
        let target = MessageTarget::at(window, x, y)?;
        let (down, up, double, held) = button.messages();
        target.post_mouse(WM_MOUSEMOVE, 0, x, y)?;
        for index in 0..count {
            crate::check_running()?;
            let message = if index == 1 { double } else { down };
            target.post_mouse(message, held, x, y)?;
            target.post_mouse(up, 0, x, y)?;
        }
        note_action();
        Ok(
            json!({"clicked": [x, y], "background": true, "delivery": "posted", "control": target.control_json()}),
        )
    }

    /// Hover: one posted mouse move, the real pointer stays where the user left it.
    pub(super) fn background_move(&mut self, params: &Value) -> Result<Value, String> {
        let (window, x, y) = self.background_point(params)?;
        self.guard_background(window)?;
        MessageTarget::at(window, x, y)?.post_mouse(WM_MOUSEMOVE, 0, x, y)?;
        note_action();
        Ok(json!({"moved": [x, y], "background": true, "delivery": "posted"}))
    }

    pub(super) fn background_drag(&mut self, params: &Value) -> Result<Value, String> {
        let window = params
            .get("window_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(background_needs_window)?;
        let info =
            describe(window as isize).ok_or_else(|| format!("window {window} is not open"))?;
        let from = (
            int_key(params, &["x1", "from_x"])?,
            int_key(params, &["y1", "from_y"])?,
        );
        let to = (
            int_key(params, &["x2", "to_x"])?,
            int_key(params, &["y2", "to_y"])?,
        );
        let points = [
            offset(from, info.left, info.top)?,
            offset(to, info.left, info.top)?,
        ];
        self.guard_background(window)?;
        let dense = densify(&points, 6)?;
        post_stroke(window, &dense)?;
        note_action();
        Ok(json!({"dragged": true, "background": true, "delivery": "posted"}))
    }

    pub(super) fn background_draw(
        &mut self,
        window: Option<i64>,
        points: &[(i32, i32)],
        step: i32,
    ) -> Result<Value, String> {
        let window = window.ok_or_else(background_needs_window)?;
        let info =
            describe(window as isize).ok_or_else(|| format!("window {window} is not open"))?;
        let mut screen = Vec::with_capacity(points.len());
        for point in points {
            screen.push(offset(*point, info.left, info.top)?);
        }
        self.guard_background(window)?;
        let dense = densify(&screen, step)?;
        post_stroke(window, &dense)?;
        note_action();
        Ok(json!({"points": dense.len(), "background": true, "delivery": "posted"}))
    }

    /// `draw_strokes` lines, already in screen pixels, posted one by one.
    pub(super) fn background_strokes(
        &mut self,
        window: i64,
        lines: &[[(i32, i32); 2]],
        step: i32,
        settle_ms: u64,
    ) -> Result<Value, String> {
        self.guard_background(window)?;
        let started = std::time::Instant::now();
        for line in lines {
            crate::check_running()?;
            if step > 0 {
                post_stroke(window, &densify(&line[..], step)?)?;
            } else {
                post_stroke(window, line)?;
            }
            crate::timing::sleep(Duration::from_millis(settle_ms))?;
        }
        note_action();
        Ok(json!({
            "strokes": lines.len(),
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "step": step,
            "background": true,
            "delivery": "posted",
        }))
    }

    pub(super) fn background_scroll(
        &mut self,
        params: &Value,
        vertical: i32,
        horizontal: i32,
    ) -> Result<Value, String> {
        let (window, x, y) = self.background_point(params)?;
        self.guard_background(window)?;
        let target = MessageTarget::at(window, x, y)?;
        // Wheel messages carry screen coordinates, not client ones.
        let screen = make_lparam(x, y);
        for (ticks, message) in [(vertical, WM_MOUSEWHEEL), (horizontal, WM_MOUSEHWHEEL)] {
            for _ in 0..ticks.unsigned_abs() {
                crate::check_running()?;
                let delta = if ticks > 0 { WHEEL_DELTA } else { -WHEEL_DELTA };
                target.post(
                    message,
                    WPARAM(((delta as u16 as usize) << 16) & 0xffff_0000),
                    screen,
                )?;
                crate::timing::sleep(Duration::from_millis(MOVE_GAP_MS * 4))?;
            }
        }
        note_action();
        Ok(json!({"scrolled": true, "background": true, "delivery": "posted"}))
    }

    /// Characters go to the control that has focus inside the target window's
    /// thread, which a background click usually sets.
    pub(super) fn background_type(&mut self, params: &Value, text: &str) -> Result<Value, String> {
        let window = params
            .get("window_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(background_needs_window)?;
        if text.chars().count() > MAX_BACKGROUND_TEXT {
            return Err(format!(
                "background typing is limited to {MAX_BACKGROUND_TEXT} characters; use set_value"
            ));
        }
        self.guard_background(window)?;
        let control = focused_control(window)?;
        for unit in background_units(text) {
            crate::check_running()?;
            post(control, WM_CHAR, WPARAM(unit as usize), LPARAM(1))?;
        }
        note_action();
        Ok(json!({"typed": text.chars().count(), "background": true, "delivery": "posted"}))
    }

    pub(super) fn background_key(&mut self, params: &Value, chord: &str) -> Result<Value, String> {
        let window = params
            .get("window_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(background_needs_window)?;
        let combo = crate::keys::parse_combo(chord)?;
        if !combo.modifiers.is_empty() {
            return Err(format!(
                "{chord} needs held modifier keys, which background mode cannot hold without touching your keyboard. Use perform_secondary_action or a menu item, or call press_key with background:false."
            ));
        }
        self.guard_background(window)?;
        let control = focused_control(window)?;
        let code = combo.key as usize;
        post(control, WM_KEYDOWN, WPARAM(code), LPARAM(1))?;
        post(
            control,
            WM_KEYUP,
            WPARAM(code),
            LPARAM(0xC000_0001u32 as isize),
        )?;
        note_action();
        Ok(json!({"pressed": chord, "background": true, "delivery": "posted"}))
    }
}

/// The deepest visible child under a screen point, found from the target
/// window itself so a covering window does not matter.
struct MessageTarget {
    control: HWND,
}

impl MessageTarget {
    fn at(window: i64, x: i32, y: i32) -> Result<Self, String> {
        let root = HWND(window as isize as *mut std::ffi::c_void);
        if unsafe { IsIconic(root) }.as_bool() {
            return Err(
                "the window is minimized; restore it (set_window_state) and leave it behind your work"
                    .into(),
            );
        }
        let mut current = root;
        for _ in 0..32 {
            let mut point = POINT { x, y };
            unsafe {
                let _ = ScreenToClient(current, &mut point);
            }
            let child = unsafe {
                ChildWindowFromPointEx(
                    current,
                    point,
                    CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT,
                )
            };
            if child.0.is_null() || child == current {
                break;
            }
            current = child;
        }
        Ok(Self { control: current })
    }

    fn post_mouse(&self, message: u32, held: usize, x: i32, y: i32) -> Result<(), String> {
        let mut point = POINT { x, y };
        unsafe {
            let _ = ScreenToClient(self.control, &mut point);
        }
        self.post(message, WPARAM(held), make_lparam(point.x, point.y))
    }

    fn post(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<(), String> {
        post(self.control, message, wparam, lparam)
    }

    fn control_json(&self) -> Value {
        json!(self.control.0 as isize as i64)
    }
}

fn post_stroke(window: i64, points: &[(i32, i32)]) -> Result<(), String> {
    let Some(first) = points.first() else {
        return Ok(());
    };
    let target = MessageTarget::at(window, first.0, first.1)?;
    target.post_mouse(WM_MOUSEMOVE, 0, first.0, first.1)?;
    target.post_mouse(WM_LBUTTONDOWN, MK_LBUTTON, first.0, first.1)?;
    let outcome = (|| -> Result<(), String> {
        for point in &points[1..] {
            crate::check_running()?;
            target.post_mouse(WM_MOUSEMOVE, MK_LBUTTON, point.0, point.1)?;
            crate::timing::sleep(Duration::from_millis(MOVE_GAP_MS))?;
        }
        Ok(())
    })();
    // Release the posted button even after Escape, so the app is not left dragging.
    let last = points.last().copied().unwrap_or(*first);
    let released = target.post_mouse(WM_LBUTTONUP, 0, last.0, last.1);
    outcome.and(released)
}

fn focused_control(window: i64) -> Result<HWND, String> {
    let root = HWND(window as isize as *mut std::ffi::c_void);
    let thread = unsafe { GetWindowThreadProcessId(root, None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread, &mut info) }.map_err(|err| err.to_string())?;
    let focus = info.hwndFocus;
    if focus.0.is_null() || owner_of(focus.0 as isize) != owner_of(window as isize) {
        return Err("no control has focus inside that window; click a field with background:true first, or use set_value".into());
    }
    Ok(focus)
}

fn post(control: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<(), String> {
    unsafe { PostMessageW(Some(control), message, wparam, lparam) }
        .map_err(|err| format!("the window refused the message: {err}"))
}

/// Pack signed client coordinates the way mouse messages expect them.
fn make_lparam(x: i32, y: i32) -> LPARAM {
    let low = (x as i16 as u16) as u32;
    let high = (y as i16 as u16) as u32;
    LPARAM(((high << 16) | low) as i32 as isize)
}

/// UTF-16 units for WM_CHAR. Newlines become Enter, carriage returns are dropped.
fn background_units(text: &str) -> Vec<u16> {
    text.chars()
        .filter(|c| *c != '\r')
        .map(|c| if c == '\n' { '\r' } else { c })
        .collect::<String>()
        .encode_utf16()
        .collect()
}

fn offset(point: (i32, i32), left: i32, top: i32) -> Result<(i32, i32), String> {
    Ok((
        point.0.checked_add(left).ok_or("x out of range")?,
        point.1.checked_add(top).ok_or("y out of range")?,
    ))
}

fn background_needs_window() -> String {
    "background mode needs a target window: pass window_id, element_index, or a window screenshotId"
        .into()
}

#[cfg(test)]
mod tests {
    use super::{background_units, default_on, make_lparam, wants_background};
    use serde_json::json;

    #[test]
    fn the_call_overrides_the_environment_default() {
        assert!(wants_background(&json!({"background": true})));
        assert!(!wants_background(&json!({"background": false})));
        assert!(default_on(Some("1")));
        assert!(default_on(Some(" ON ")));
        assert!(!default_on(Some("0")));
        assert!(!default_on(None));
    }

    #[test]
    fn coordinates_pack_as_signed_words() {
        assert_eq!(make_lparam(10, 20).0, (20 << 16) | 10);
        let packed = make_lparam(-5, 7).0 as u32;
        assert_eq!((packed & 0xffff) as u16 as i16, -5);
        assert_eq!((packed >> 16) as u16 as i16, 7);
    }

    #[test]
    fn newlines_become_enter() {
        assert_eq!(background_units("a\r\nb"), vec![97, 13, 98]);
        assert_eq!(background_units("ğ"), vec![0x011f]);
        assert_eq!(background_units("😀").len(), 2);
    }
}
