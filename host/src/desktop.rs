//! Desktop operations: windows, input, screenshots, and the accessibility tree.
//!
//! A window screenshot uses that window's own pixels when Windows graphics
//! capture is available, including while another window covers it. Pointer
//! actions then bring that window forward. A point that stays covered is not clicked;
//! the refusal names the covering process and attaches a fresh target screenshot.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use uiautomation::inputs::{Mouse, MouseButton};
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UIRangeValuePattern, UIScrollItemPattern,
    UIScrollPattern, UISelectionItemPattern, UITextPattern, UIValuePattern,
};
use uiautomation::screenshots::Screenshot;
use uiautomation::types::{ControlType, ExpandCollapseState, Point, Rect};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetForegroundWindow, GetSystemMetrics, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SetForegroundWindow, ShowWindow, GA_ROOTOWNER, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, SW_SHOWNORMAL,
};

const MAX_NODES: usize = 400;
const MAX_PASTE_CHARS: usize = 200_000;
const MAX_DEPTH: usize = 14;
const MAX_CHILDREN: usize = 64;
const STATE_FLOOR_MS: u64 = 1000;
const STATE_EXTRA_MS: u64 = 5000;
const STATE_POLL_MS: u64 = 200;
const STATE_WALK_DEPTH: usize = 8;
const STATE_WALK_NODES: usize = 64;

struct CachedNode {
    element: UIElement,
    line: String,
    focused: bool,
    selected: bool,
}

/// Window handle plus the region's left, top, right, and bottom in physical pixels.
type RegionFrame = (i64, i32, i32, i32, i32);

struct RegionCapture {
    hwnd: i64,
    region: (i32, i32, i32, i32),
    anchor: (i32, i32, i32, i32),
    scale: f32,
    role: &'static str,
    monitor: Option<String>,
    z_index: i32,
    /// `window` when the pixels come from the window itself, otherwise `visible`.
    capture: &'static str,
}

/// One bounded screenshot. Coordinates for its id are physical pixels inside this
/// region, whose screen origin may sit on any monitor. Sibling regions from the
/// same capture share `generation`. A later capture of the same target replaces them.
struct ShotRecord {
    id: String,
    hwnd: i64,
    generation: u64,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    anchor_left: i32,
    anchor_top: i32,
    anchor_right: i32,
    anchor_bottom: i32,
    virtual_left: i32,
    virtual_top: i32,
    virtual_width: i32,
    virtual_height: i32,
}

#[derive(Clone)]
struct Monitor {
    device: String,
    primary: bool,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    work_left: i32,
    work_top: i32,
    work_right: i32,
    work_bottom: i32,
    dpi_x: u32,
    dpi_y: u32,
}

pub struct Session {
    automation: UIAutomation,
    walker: UITreeWalker,
    cache_hwnd: isize,
    nodes: Vec<CachedNode>,
    shots: Vec<ShotRecord>,
    shot_seq: u64,
    generation: u64,
}

impl Session {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let automation = UIAutomation::new().map_err(|err| err.to_string())?;
        let walker = automation
            .get_control_view_walker()
            .map_err(|err| err.to_string())?;
        Ok(Self {
            automation,
            walker,
            cache_hwnd: 0,
            nodes: Vec::new(),
            shots: Vec::new(),
            shot_seq: 0,
            generation: 0,
        })
    }

    pub fn list_windows(&self, query: &str) -> Result<Value, String> {
        let needle = query.to_ascii_lowercase();
        let mut rows = Vec::new();
        for hwnd in top_level_windows() {
            let Some(info) = describe(hwnd) else { continue };
            if info.pid == std::process::id() {
                continue;
            }
            if !needle.is_empty() && !info.title.to_ascii_lowercase().contains(&needle) {
                continue;
            }
            rows.push(info.to_json());
        }
        Ok(Value::Array(rows))
    }

    pub fn list_apps(&self, query: &str, include_start_menu: bool) -> Result<Value, String> {
        let windows = self.list_windows("")?;
        let mut groups: Vec<Value> = Vec::new();
        if let Some(items) = windows.as_array() {
            for window in items {
                let app = window
                    .get("app")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = window.get("title").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(group) = groups
                    .iter_mut()
                    .find(|group| group.get("id").and_then(|v| v.as_str()) == Some(app.as_str()))
                {
                    if let Some(list) = group.get_mut("windows").and_then(|v| v.as_array_mut()) {
                        list.push(window.clone());
                    }
                    let _ = title;
                } else {
                    let display = std::path::Path::new(&app)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or(title);
                    groups.push(json!({
                        "id": app,
                        "displayName": display,
                        "isRunning": true,
                        "windows": [window],
                    }));
                }
            }
        }
        if include_start_menu {
            groups.extend(start_menu(query));
        }
        let needle = query.to_ascii_lowercase();
        if !needle.is_empty() {
            groups.retain(|app| {
                let name = app
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let id = app
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                name.contains(&needle) || id.contains(&needle)
            });
        }
        Ok(Value::Array(groups))
    }

    pub fn get_window(&self, window_id: i64) -> Result<Value, String> {
        describe(window_id as isize)
            .map(|info| info.to_json())
            .ok_or_else(|| format!("window {window_id} is not open"))
    }

    pub fn activate(&mut self, window_id: i64) -> Result<Value, String> {
        self.guard_input(window_id)?;
        focus(window_id as isize)?;
        note_action();
        self.get_window(window_id)
    }

    pub fn window_state(
        &mut self,
        window_id: i64,
        shot: bool,
        text: bool,
        scale: f32,
    ) -> Result<Value, String> {
        self.ensure_allowed(window_id)?;
        if shot {
            self.guard_input(window_id)?;
        }
        let waited_ms = self.settle_before_observation(window_id as isize)?;
        let info = describe(window_id as isize)
            .ok_or_else(|| format!("window {window_id} is not open"))?;
        let mut result = json!({
            "window": info.to_json(),
            "accessibility": null,
            "waited_ms": waited_ms,
        });
        if text {
            let tree = self.snapshot(window_id as isize)?;
            result["accessibility"] = accessibility_payload(&self.nodes, &tree);
        }
        if shot {
            let views = self.capture_window_views(&info, scale)?;
            result["screenshot"] = views.0;
            result["screenshots"] = views.1;
        }
        Ok(result)
    }

    /// Whole-desktop (or region) screenshot. Coordinates in the image map back with
    /// `screen = origin + pixel / scale`, where origin is the region's top-left.
    pub fn screenshot(&mut self, params: &Value) -> Result<Value, String> {
        let (origin_x, origin_y, width, height) = virtual_screen();
        let scale = params.get("scale").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
        let requested = match params.get("region").and_then(|v| v.as_array()) {
            Some(region) if region.len() == 4 => {
                let value = |index: usize| {
                    region[index]
                        .as_i64()
                        .ok_or_else(|| "region must hold four integers".to_string())
                };
                let left = (value(0)? as i32).clamp(origin_x, origin_x + width);
                let top = (value(1)? as i32).clamp(origin_y, origin_y + height);
                let right = (value(2)? as i32).clamp(origin_x, origin_x + width);
                let bottom = (value(3)? as i32).clamp(origin_y, origin_y + height);
                if right <= left || bottom <= top {
                    return Err("region is empty after clamping to the screen".into());
                }
                Some((left, top, right, bottom))
            }
            Some(_) => return Err("region must be [left, top, right, bottom]".into()),
            None => None,
        };
        // Wait before the generation bump. Escape during the wait must not stale
        // the previous screenshotId.
        let waited_ms = self.settle_before_observation(0)?;
        self.generation = self.generation.saturating_add(1);
        let (left, top, right, bottom) =
            requested.unwrap_or((origin_x, origin_y, origin_x + width, origin_y + height));
        let primary = self.capture_region(RegionCapture {
            hwnd: 0,
            region: (left, top, right, bottom),
            anchor: (left, top, right, bottom),
            scale,
            role: "desktop",
            monitor: None,
            z_index: 0,
            capture: "visible",
        })?;
        // The combined virtual desktop stays screenshots[0] so its id matches the
        // image the model sees. A second monitor is its own region; a single
        // display that already equals the virtual screen is not captured twice.
        let mut regions = vec![primary.clone()];
        if requested.is_none() {
            for monitor in monitors()? {
                let overlap = (monitor.left, monitor.top, monitor.right, monitor.bottom);
                if overlap == (left, top, right, bottom) {
                    continue;
                }
                regions.push(self.capture_region(RegionCapture {
                    hwnd: 0,
                    region: overlap,
                    anchor: overlap,
                    scale,
                    role: "monitor",
                    monitor: Some(monitor.device),
                    z_index: 0,
                    capture: "visible",
                })?);
            }
        }
        Ok(json!({
            "waited_ms": waited_ms,
            "screenshot": {
                "id": primary["id"],
                "png_base64": primary["png_base64"],
                "scale": primary["scale"],
                "width": primary["width"],
                "height": primary["height"],
                "origin": [left, top],
                "note": format!(
                    "region pixel (px,py) -> screen ({left}+px, {top}+py). Image pixels are region pixels times scale {scale:.3}; divide image pixels by scale. screenshots[0] is this image. Later entries are other monitors; pass that entry's id as screenshotId and use pixels inside that region.",
                    left = left, top = top, scale = primary["scale"].as_f64().unwrap_or(1.0)
                ),
            },
            "screenshots": regions.iter().map(public_shot).collect::<Vec<_>>(),
        }))
    }

    pub fn click(&mut self, params: &Value) -> Result<Value, String> {
        let hinted = params.get("window_id").and_then(|v| v.as_i64());
        let frame = self.bound_frame(params, hinted)?;
        let (x, y, window_id) = if params
            .get("element_index")
            .and_then(|v| v.as_i64())
            .is_some_and(|index| index >= 0)
        {
            let mut stash = None;
            let params = params_with_window(
                params,
                frame.map(|item| item.0).filter(|hwnd| *hwnd != 0),
                &mut stash,
            );
            self.resolve_point(params)?
        } else if let Some(frame) = frame {
            let point = point_in_frame(int_key(params, &["x"])?, int_key(params, &["y"])?, frame)?;
            (point.0, point.1, point.2)
        } else {
            self.resolve_point(params)?
        };
        if let Some(id) = window_id {
            self.guard_input(id)?;
            if let Some(refusal) = self.pointer_target(id as isize, x, y)? {
                return Ok(refusal);
            }
        } else {
            self.guard_point(x, y)?;
        }
        let button = params
            .get("button")
            .or_else(|| params.get("mouse_button"))
            .and_then(|v| v.as_str())
            .unwrap_or("left");
        let count = params
            .get("count")
            .or_else(|| params.get("click_count"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1);
        let mouse = Mouse::new().move_time(40).interval(16);
        let button = match button {
            "left" | "l" => MouseButton::LEFT,
            "right" | "r" => MouseButton::RIGHT,
            "middle" | "m" => MouseButton::MIDDLE,
            _ => return Err("unknown mouse button".into()),
        };
        if !(1..=2).contains(&count) {
            return Err("click count must be 1 or 2".into());
        }
        crate::motion::move_pointer(x, y, instant_motion(params)?)?;
        for index in 0..count {
            ensure_point_allowed(x, y)?;
            crate::check_running()?;
            mouse.click_button(button).map_err(|e| e.to_string())?;
            if index + 1 < count {
                crate::timing::sleep(Duration::from_millis(60))?;
            }
        }
        crate::overlay::signal(x, y, true);
        note_injection();
        Ok(json!({"clicked": [x, y]}))
    }

    pub fn move_to(&mut self, params: &Value) -> Result<Value, String> {
        let hinted = params.get("window_id").and_then(|v| v.as_i64());
        let frame = self.bound_frame(params, hinted)?;
        let (x, y, window_id) = if params
            .get("element_index")
            .and_then(|v| v.as_i64())
            .is_some_and(|index| index >= 0)
        {
            self.resolve_point(params)?
        } else if let Some(frame) = frame {
            let point = point_in_frame(int_key(params, &["x"])?, int_key(params, &["y"])?, frame)?;
            (point.0, point.1, point.2)
        } else {
            self.resolve_point(params)?
        };
        if let Some(id) = window_id {
            self.guard_input(id)?;
            if let Some(refusal) = self.pointer_target(id as isize, x, y)? {
                return Ok(refusal);
            }
        } else {
            self.guard_point(x, y)?;
        }
        crate::motion::move_pointer(x, y, instant_motion(params)?)?;
        note_injection();
        crate::overlay::signal(x, y, false);
        Ok(json!({"moved": [x, y]}))
    }

    pub fn drag(&mut self, params: &Value) -> Result<Value, String> {
        let hinted = params.get("window_id").and_then(|v| v.as_i64());
        let frame = self.bound_frame(params, hinted)?;
        let x1 = int_key(params, &["x1", "from_x"])?;
        let y1 = int_key(params, &["y1", "from_y"])?;
        let x2 = int_key(params, &["x2", "to_x"])?;
        let y2 = int_key(params, &["y2", "to_y"])?;
        let (start, end) = if let Some(frame) = frame {
            let start = point_in_frame(x1, y1, frame)?;
            if frame.0 != 0 {
                self.guard_input(frame.0)?;
                if let Some(refusal) = self.pointer_target(frame.0 as isize, start.0, start.1)? {
                    return Ok(refusal);
                }
            } else {
                self.guard_point(start.0, start.1)?;
            }
            (
                Point::new(start.0, start.1),
                Point::new(
                    frame.1.checked_add(x2).ok_or("drag x out of range")?,
                    frame.2.checked_add(y2).ok_or("drag y out of range")?,
                ),
            )
        } else {
            match hinted {
                Some(id) => {
                    self.guard_input(id)?;
                    let info =
                        describe(id as isize).ok_or_else(|| format!("window {id} is not open"))?;
                    if let Some(refusal) = self.pointer_target(
                        id as isize,
                        info.left.saturating_add(x1),
                        info.top.saturating_add(y1),
                    )? {
                        return Ok(refusal);
                    }
                    (
                        Point::new(
                            info.left.checked_add(x1).ok_or("drag x out of range")?,
                            info.top.checked_add(y1).ok_or("drag y out of range")?,
                        ),
                        Point::new(
                            info.left.checked_add(x2).ok_or("drag x out of range")?,
                            info.top.checked_add(y2).ok_or("drag y out of range")?,
                        ),
                    )
                }
                None => {
                    self.guard_point(x1, y1)?;
                    (Point::new(x1, y1), Point::new(x2, y2))
                }
            }
        };
        let dense = densify(
            &[(start.get_x(), start.get_y()), (end.get_x(), end.get_y())],
            6,
        )?;
        stroke(&dense)?;
        Ok(json!({"dragged": true}))
    }

    pub fn draw(&mut self, params: &Value) -> Result<Value, String> {
        let window_id = params.get("window_id").and_then(|v| v.as_i64());
        let raw = params
            .get("points")
            .and_then(|v| v.as_array())
            .ok_or("draw needs a points array")?;
        if raw.len() > 4096 {
            return Err("at most 4096 points per stroke".into());
        }
        let mut points = Vec::new();
        for item in raw {
            let pair = item.as_array().ok_or("each point is [x, y]")?;
            if pair.len() != 2 {
                return Err("each point must contain exactly x and y".into());
            }
            let x = i32::try_from(pair[0].as_i64().ok_or("point x")?)
                .map_err(|_| "point x out of range")?;
            let y = i32::try_from(pair[1].as_i64().ok_or("point y")?)
                .map_err(|_| "point y out of range")?;
            points.push((x, y));
        }
        if points.len() < 2 {
            return Err("a stroke needs at least two points".into());
        }
        let step = match params.get("step") {
            None => 6,
            Some(value) => i32::try_from(value.as_i64().ok_or("step must be an integer")?)
                .map_err(|_| "step out of range")?,
        };
        if !(1..=40).contains(&step) {
            return Err("step must be between 1 and 40".into());
        }
        if let Some(id) = window_id {
            self.guard_input(id)?;
            let info = describe(id as isize).ok_or_else(|| format!("window {id} is not open"))?;
            if let Some(refusal) = self.pointer_target(
                id as isize,
                info.left.saturating_add(points[0].0),
                info.top.saturating_add(points[0].1),
            )? {
                return Ok(refusal);
            }
            for point in &mut points {
                point.0 = point
                    .0
                    .checked_add(info.left)
                    .ok_or("stroke x out of range")?;
                point.1 = point
                    .1
                    .checked_add(info.top)
                    .ok_or("stroke y out of range")?;
            }
        } else {
            self.guard_point(points[0].0, points[0].1)?;
        }
        let dense = densify(&points, step)?;
        stroke(&dense)?;
        Ok(json!({"points": dense.len()}))
    }

    /// Independent straight strokes, with one validated target/idle check.
    /// Used by image-to-stroke plans; never opens, pastes or loads an image.
    pub fn draw_strokes(&mut self, params: &Value) -> Result<Value, String> {
        let id = params
            .get("window_id")
            .and_then(|v| v.as_i64())
            .ok_or("draw_strokes requires window_id")?;
        let info = describe(id as isize).ok_or("stroke target is not open")?;
        let width = info.right - info.left;
        let height = info.bottom - info.top;
        let raw = params
            .get("strokes")
            .and_then(|v| v.as_array())
            .ok_or("strokes must be an array")?;
        if raw.is_empty() || raw.len() > 2000 {
            return Err("provide 1 to 2000 strokes".into());
        }
        let mut lines = Vec::with_capacity(raw.len());
        for value in raw {
            let values = value.as_array().ok_or("each stroke is [x1,y1,x2,y2]")?;
            if values.len() != 4 {
                return Err("each stroke needs four coordinates".into());
            }
            let mut p = [0; 4];
            for (index, value) in values.iter().enumerate() {
                p[index] = i32::try_from(
                    value
                        .as_i64()
                        .ok_or("stroke coordinate must be an integer")?,
                )
                .map_err(|_| "stroke coordinate out of range")?;
                let limit = if index % 2 == 0 { width } else { height };
                if p[index] < 0 || p[index] >= limit {
                    return Err("stroke leaves the target window".into());
                }
                p[index] = p[index]
                    .checked_add(if index % 2 == 0 { info.left } else { info.top })
                    .ok_or("stroke coordinate overflow")?;
            }
            lines.push([(p[0], p[1]), (p[2], p[3])]);
        }
        let hold_ms = params
            .get("hold_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .min(500) as u32;
        let settle_ms = params
            .get("settle_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(3)
            .min(1000);
        // 0 sends only the two end points (fastest, fine for Paint). A positive step walks the
        // pointer in steps of that many pixels, which apps that sample the cursor late need.
        let step = params
            .get("step")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .clamp(0, 200) as i32;
        self.guard_input(id)?;
        if let Some(refusal) = self.pointer_target(id as isize, lines[0][0].0, lines[0][0].1)? {
            return Ok(refusal);
        }
        let start = Instant::now();
        STROKE_HOLD_MS.store(hold_ms, std::sync::atomic::Ordering::Relaxed);
        let outcome = (|| -> Result<(), String> {
            for line in &lines {
                crate::check_running()?;
                if step > 0 {
                    stroke(&densify(&line[..], step)?)?;
                } else {
                    stroke(line)?;
                }
                // Let the target process consume the down/move/up sequence.
                crate::timing::sleep(Duration::from_millis(settle_ms))?;
            }
            Ok(())
        })();
        STROKE_HOLD_MS.store(0, std::sync::atomic::Ordering::Relaxed);
        outcome?;
        Ok(
            json!({"strokes":lines.len(),"elapsed_ms":start.elapsed().as_millis(),"hold_ms":hold_ms,"settle_ms":settle_ms,"step":step}),
        )
    }

    pub fn scroll(&mut self, params: &Value) -> Result<Value, String> {
        let hinted = params.get("window_id").and_then(|v| v.as_i64());
        let frame = self.bound_frame(params, hinted)?;
        let (x, y, window_id) = if let Some(frame) = frame {
            let point = point_in_frame(int_key(params, &["x"])?, int_key(params, &["y"])?, frame)?;
            (point.0, point.1, point.2)
        } else {
            self.resolve_point(params)?
        };
        if let Some(id) = window_id {
            self.guard_input(id)?;
            if let Some(refusal) = self.pointer_target(id as isize, x, y)? {
                return Ok(refusal);
            }
        } else {
            self.guard_point(x, y)?;
        }
        let instant = instant_motion(params)?;
        let vertical = if params.get("scrollY").is_some() || params.get("scroll_y").is_some() {
            -wheel_ticks(int_key(params, &["scrollY", "scroll_y"]).unwrap_or(0))
        } else if params
            .get("horizontal")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            0
        } else {
            int_key(params, &["ticks"]).unwrap_or(0)
        };
        let horizontal = if params.get("scrollX").is_some() || params.get("scroll_x").is_some() {
            wheel_ticks(int_key(params, &["scrollX", "scroll_x"]).unwrap_or(0))
        } else if params
            .get("horizontal")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            int_key(params, &["ticks"]).unwrap_or(0)
        } else {
            0
        };
        if vertical == 0 && horizontal == 0 {
            return Err("provide a non-zero scroll delta".into());
        }
        if vertical.unsigned_abs() > 100 || horizontal.unsigned_abs() > 100 {
            return Err("at most 100 wheel ticks per axis per request".into());
        }
        crate::motion::move_pointer(x, y, instant)?;
        ensure_point_allowed(x, y)?;
        crate::check_running()?;
        paced_wheel(vertical, false, instant)?;
        paced_wheel(horizontal, true, instant)?;
        note_injection();
        crate::overlay::signal(x, y, false);
        Ok(json!({"scrolled": true}))
    }

    pub fn type_text(&mut self, params: &Value) -> Result<Value, String> {
        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or("missing text")?;
        if let Some(id) = params.get("window_id").and_then(|v| v.as_i64()) {
            self.guard_input(id)?;
            focus(id as isize)?;
        } else {
            self.guard_foreground()?;
        }
        type_unicode(text)?;
        note_injection();
        Ok(json!({"typed": text.chars().count()}))
    }

    pub fn press_key(&mut self, params: &Value) -> Result<Value, String> {
        let chord = params
            .get("key")
            .or_else(|| params.get("keys"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some(id) = params.get("window_id").and_then(|v| v.as_i64()) {
            self.guard_input(id)?;
            focus(id as isize)?;
        } else {
            self.guard_foreground()?;
        }
        let combo = crate::keys::parse_combo(chord)?;
        send_combo(&combo)?;
        note_injection();
        crate::overlay::touch();
        Ok(json!({"pressed": chord}))
    }

    pub fn set_value(&mut self, params: &Value) -> Result<Value, String> {
        self.guard_cached_target(params)?;
        let index = int_key(params, &["element_index"])? as usize;
        let value = params
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or("missing value")?
            .to_string();
        let element = self.cached(index)?;
        let _ = element.set_focus();
        let pattern: UIValuePattern = element
            .get_pattern()
            .map_err(|_| format!("element [{index}] is not editable"))?;
        pattern.set_value(&value).map_err(|err| err.to_string())?;
        note_action();
        Ok(json!({"set": index}))
    }

    pub fn secondary(&mut self, params: &Value) -> Result<Value, String> {
        self.guard_cached_target(params)?;
        let index = int_key(params, &["element_index"])? as usize;
        let action = params
            .get("action")
            .or_else(|| params.get("secondary"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let element = self.cached(index)?;
        let _ = element.set_focus();
        match canonical(action)?.as_str() {
            "invoke" => {
                let pattern: UIInvokePattern =
                    element.get_pattern().map_err(|err| err.to_string())?;
                pattern.invoke().map_err(|err| err.to_string())?;
            }
            "expand" => {
                let pattern: UIExpandCollapsePattern =
                    element.get_pattern().map_err(|err| err.to_string())?;
                pattern.expand().map_err(|err| err.to_string())?;
            }
            "collapse" => {
                let pattern: UIExpandCollapsePattern =
                    element.get_pattern().map_err(|err| err.to_string())?;
                pattern.collapse().map_err(|err| err.to_string())?;
            }
            "raise" => {
                let pattern: UIScrollItemPattern =
                    element.get_pattern().map_err(|err| err.to_string())?;
                pattern.scroll_into_view().map_err(|err| err.to_string())?;
            }
            "scroll_up" | "scroll_down" | "scroll_left" | "scroll_right" => {
                let pattern: UIScrollPattern =
                    element.get_pattern().map_err(|err| err.to_string())?;
                scroll_pattern(&pattern, action)?;
            }
            other => return Err(format!("unknown action {other}")),
        }
        note_action();
        Ok(json!({"ran": action, "element": index}))
    }

    pub fn paste(&mut self, params: &Value) -> Result<Value, String> {
        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or("missing text")?;
        if text.chars().count() > MAX_PASTE_CHARS {
            return Err(format!("paste text exceeds {MAX_PASTE_CHARS} characters"));
        }
        let format = paste_format(params)?;
        let (plain, html) = match format {
            "html" => (html_to_plain(text), Some(text.to_string())),
            "md" => (text.to_string(), Some(markdown_to_html(text))),
            _ => (text.to_string(), None),
        };
        if let Some(id) = params.get("window_id").and_then(|v| v.as_i64()) {
            self.guard_input(id)?;
            focus(id as isize)?;
        } else {
            self.guard_foreground()?;
        }
        if let Some(html) = html.as_deref() {
            crate::guard::prepare()?;
            crate::guard::begin_action();
            write_rich_clipboard(&plain, html)?;
        } else {
            self.clipboard_write(&plain)?;
        }
        crate::check_running()?;
        send_combo(&crate::keys::parse_combo("ctrl+v")?)?;
        note_injection();
        Ok(json!({"pasted": text.chars().count(), "format": format}))
    }

    pub fn clipboard_read(&self) -> Result<Value, String> {
        let text = read_clipboard()?;
        Ok(json!({"text": text}))
    }

    pub fn clipboard_write(&self, text: &str) -> Result<Value, String> {
        crate::guard::prepare()?;
        crate::guard::begin_action();
        write_clipboard(text)?;
        Ok(json!({"wrote": text.chars().count()}))
    }

    pub fn launch(&self, app: &str) -> Result<Value, String> {
        let target = if app.trim().is_empty() {
            return Err("launch_app needs an app id or name".into());
        } else {
            app.trim()
        };
        ensure_token(target)?;
        crate::guard::prepare()?;
        crate::guard::begin_action();
        let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
        let open: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        let code = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(open.as_ptr()),
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if (code.0 as isize) <= 32 {
            return Err(format!(
                "could not launch {target} (ShellExecute {})",
                code.0 as isize
            ));
        }
        note_action();
        Ok(json!({"launched": target}))
    }

    pub fn batch(&mut self, actions: Value) -> Result<Value, String> {
        let list = actions.as_array().ok_or("batch needs an actions array")?;
        if list.len() > 50 {
            return Err("at most 50 actions per batch".into());
        }
        let mut results = Vec::new();
        for (index, action) in list.iter().enumerate() {
            if let Err(error) = crate::check_running() {
                results.push(json!({"index":index,"ok":false,"error":error}));
                break;
            }
            let name = action.get("action").and_then(|v| v.as_str()).unwrap_or("");
            let outcome = match name {
                "click" | "double_click" | "right_click" => {
                    let mut owned = action.clone();
                    if name == "double_click" {
                        owned["count"] = json!(2);
                    }
                    if name == "right_click" {
                        owned["button"] = json!("right");
                    }
                    self.click(&owned)
                }
                "move" => self.move_to(action),
                "drag" => self.drag(action),
                "draw" => self.draw(action),
                "scroll" => self.scroll(action),
                "type" | "type_text" => self.type_text(action),
                "key" | "press_key" => self.press_key(action),
                "set_value" => self.set_value(action),
                "secondary" => self.secondary(action),
                "paste" => self.paste(action),
                "focus_window" => {
                    let title = action.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let windows = self.list_windows(title)?;
                    let id = windows
                        .as_array()
                        .and_then(|rows| rows.first())
                        .and_then(|row| row.get("id"))
                        .and_then(|v| v.as_i64());
                    match id {
                        Some(id) => self.activate(id),
                        None => Err(format!("no visible window matches {title:?}")),
                    }
                }
                "wait" => {
                    crate::guard::begin_action();
                    let seconds = action
                        .get("seconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5)
                        .clamp(0.0, 10.0);
                    crate::cancellable_wait(Duration::from_secs_f64(seconds))
                        .map(|()| json!({"waited": seconds}))
                }
                other => Err(format!("unknown action {other:?}")),
            };
            match outcome {
                Ok(value) if is_covered_refusal(&value) => {
                    let compact = batch_covered_result(&value);
                    let message = compact
                        .get("message")
                        .and_then(|item| item.as_str())
                        .unwrap_or("point stayed covered; no input was sent")
                        .to_string();
                    results.push(
                        json!({"index": index, "ok": false, "error": message, "result": compact}),
                    );
                    break;
                }
                Ok(value) => results.push(json!({"index": index, "ok": true, "result": value})),
                Err(err) => {
                    results.push(json!({"index": index, "ok": false, "error": err}));
                    break;
                }
            }
        }
        Ok(Value::Array(results))
    }

    /// Wait out a recent action before reading the screen. Does not call the
    /// idle gate. hwnd 0 samples the foreground window. Escape aborts the wait.
    fn settle_before_observation(&self, hwnd: isize) -> Result<u64, String> {
        let started = Instant::now();
        let Some(elapsed) = action_age_ms() else {
            return Ok(0);
        };
        if elapsed >= STATE_FLOOR_MS + STATE_EXTRA_MS {
            return Ok(0);
        }
        let floor = STATE_FLOOR_MS.saturating_sub(elapsed);
        if floor > 0 {
            crate::timing::sleep(Duration::from_millis(floor))?;
        }
        let extra_ms = STATE_EXTRA_MS.saturating_sub(elapsed.saturating_sub(STATE_FLOOR_MS));
        let extra_deadline = Instant::now() + Duration::from_millis(extra_ms);
        let mut seen = Vec::new();
        loop {
            let sample = self.read_state_sample(hwnd);
            let before = observation_wait_ms(elapsed, &seen);
            seen.push(sample);
            let after = observation_wait_ms(elapsed, &seen);
            if after == before || Instant::now() >= extra_deadline {
                break;
            }
            let remaining = extra_deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64;
            let slice = (after - before).min(remaining);
            if slice == 0 {
                break;
            }
            crate::timing::sleep(Duration::from_millis(slice))?;
        }
        Ok(started.elapsed().as_millis() as u64)
    }

    /// Fingerprint the window without touching the accessibility snapshot cache.
    /// A failed tree read keeps the window rectangle and is not treated as busy,
    /// so two failures look stable.
    fn read_state_sample(&self, hwnd: isize) -> StateSample {
        let target = if hwnd == 0 {
            unsafe { GetForegroundWindow().0 as isize }
        } else {
            hwnd
        };
        if target == 0 {
            return StateSample {
                busy: false,
                signature: 0,
            };
        }
        let signature = window_rect_signature(target);
        let handle = uiautomation::types::Handle::from(HWND(target as *mut std::ffi::c_void));
        let Ok(root) = self.automation.element_from_handle(handle) else {
            return StateSample {
                busy: false,
                signature,
            };
        };
        let mut signature = signature;
        let mut busy = false;
        let mut count = 0usize;
        fold_state(
            &self.walker,
            &root,
            0,
            &mut signature,
            &mut busy,
            &mut count,
        );
        StateSample { busy, signature }
    }

    fn snapshot(&mut self, hwnd: isize) -> Result<String, String> {
        let handle = uiautomation::types::Handle::from(HWND(hwnd as *mut std::ffi::c_void));
        let element = self
            .automation
            .element_from_handle(handle)
            .map_err(|err| err.to_string())?;
        self.nodes.clear();
        self.cache_hwnd = hwnd;
        walk(&self.walker, &element, 0, &mut self.nodes)?;
        let mut lines: Vec<&str> = self.nodes.iter().map(|node| node.line.as_str()).collect();
        if self.nodes.len() >= MAX_NODES {
            lines.push("(truncated at 400 elements)");
        }
        Ok(lines.join("\n"))
    }

    fn guard_cached_target(&self, params: &Value) -> Result<(), String> {
        if let Some(id) = params.get("window_id").and_then(|v| v.as_i64()) {
            if id != self.cache_hwnd as i64 {
                return Err(
                    "element belongs to another window; call get_window_state again".into(),
                );
            }
        }
        self.guard_input(self.cache_hwnd as i64)
    }

    fn cached(&self, index: usize) -> Result<&UIElement, String> {
        self.nodes.get(index).map(|node| &node.element).ok_or_else(|| {
            "element_index is not in the latest accessibility snapshot; call get_window_state again".into()
        })
    }

    fn resolve_point(&self, params: &Value) -> Result<(i32, i32, Option<i64>), String> {
        if let Some(index) = params.get("element_index").and_then(|v| v.as_i64()) {
            if index >= 0 {
                self.guard_cached_target(params)?;
                let element = self.cached(index as usize)?;
                let rect = element
                    .get_bounding_rectangle()
                    .map_err(|err| err.to_string())?;
                let x = (rect.get_left() + rect.get_right()) / 2;
                let y = (rect.get_top() + rect.get_bottom()) / 2;
                return Ok((x, y, Some(self.cache_hwnd as i64)));
            }
        }
        let x = int_key(params, &["x"])?;
        let y = int_key(params, &["y"])?;
        if let Some(window_id) = params.get("window_id").and_then(|v| v.as_i64()) {
            if window_id != 0 {
                let info = describe(window_id as isize)
                    .ok_or_else(|| format!("window {window_id} is not open"))?;
                if x < 0 || y < 0 || x >= info.right - info.left || y >= info.bottom - info.top {
                    return Err("point leaves the target window".into());
                }
                return Ok((
                    info.left.checked_add(x).ok_or("point x out of range")?,
                    info.top.checked_add(y).ok_or("point y out of range")?,
                    Some(window_id),
                ));
            }
        }
        Ok((x, y, None))
    }

    fn guard_input(&self, window_id: i64) -> Result<(), String> {
        crate::guard::prepare()?;
        self.ensure_allowed(window_id)?;
        crate::guard::begin_action();
        Ok(())
    }
    fn guard_point(&self, x: i32, y: i32) -> Result<(), String> {
        crate::guard::prepare()?;
        if allow_list_on() {
            ensure_token(&process_image_of(window_at(x, y)))?;
        }
        crate::guard::begin_action();
        Ok(())
    }
    fn guard_foreground(&self) -> Result<(), String> {
        crate::guard::prepare()?;
        if allow_list_on() {
            let hwnd = unsafe { GetForegroundWindow() };
            ensure_token(&process_image_of(hwnd.0 as isize))?;
        }
        crate::guard::begin_action();
        Ok(())
    }

    fn ensure_allowed(&self, window_id: i64) -> Result<(), String> {
        if !allow_list_on() {
            return Ok(());
        }
        let info = describe(window_id as isize)
            .ok_or_else(|| format!("window {window_id} is not open"))?;
        ensure_token(&info.app)
    }

    /// `screenshotId` selects one bounded region from the latest capture. `x` and `y`
    /// are physical pixels inside that region, so a monitor slice or owned popup
    /// does not inherit the parent window's origin.
    fn bound_frame(
        &self,
        params: &Value,
        window_id: Option<i64>,
    ) -> Result<Option<RegionFrame>, String> {
        let Some(raw) = screenshot_id_param(params)? else {
            return Ok(None);
        };
        let hwnd = check_shot(&self.shots, raw, window_id, None)?;
        let shot = self
            .shots
            .iter()
            .rev()
            .find(|shot| shot.id == raw.trim())
            .ok_or_else(|| {
                format!(
                    "screenshotId {} is not cached; call get_window_state again",
                    raw.trim()
                )
            })?;
        if hwnd != 0 {
            let info =
                describe(hwnd as isize).ok_or_else(|| format!("window {hwnd} is not open"))?;
            check_shot(
                &self.shots,
                raw,
                Some(hwnd),
                Some((info.left, info.top, info.right, info.bottom)),
            )?;
        } else {
            let (left, top, width, height) = virtual_screen();
            if shot.virtual_left != left
                || shot.virtual_top != top
                || shot.virtual_width != width
                || shot.virtual_height != height
            {
                return Err(
                    "display layout changed since screenshotId was captured; take a new screenshot"
                        .into(),
                );
            }
        }
        Ok(Some((hwnd, shot.left, shot.top, shot.right, shot.bottom)))
    }

    fn capture_window_views(
        &mut self,
        info: &WindowInfo,
        scale: f32,
    ) -> Result<(Value, Value), String> {
        crate::guard::desktop_available()?;
        self.generation = self.generation.saturating_add(1);
        let anchor = (info.left, info.top, info.right, info.bottom);
        let image = crate::window_capture::capture_window(info.hwnd)?;
        let capture = if image.own_content {
            "window"
        } else {
            "visible"
        };
        let content = (
            image.left,
            image.top,
            image.left.saturating_add(image.width as i32),
            image.top.saturating_add(image.height as i32),
        );
        let mut shots = vec![self.capture_from_image(
            &image,
            RegionCapture {
                hwnd: info.hwnd as i64,
                region: content,
                anchor,
                scale,
                role: "window",
                monitor: None,
                z_index: 0,
                capture,
            },
        )?];
        if let Ok(list) = monitors() {
            for (monitor, overlap) in spanning_slices(anchor, &list) {
                if let Ok(slice) = self.capture_from_image(
                    &image,
                    RegionCapture {
                        hwnd: info.hwnd as i64,
                        region: overlap,
                        anchor,
                        scale,
                        role: "monitor",
                        monitor: Some(monitor.device),
                        z_index: 0,
                        capture,
                    },
                ) {
                    shots.push(slice);
                }
            }
        }
        for (index, hwnd) in transient_windows(info.hwnd).into_iter().enumerate() {
            let Some(popup) = describe(hwnd) else {
                continue;
            };
            if popup.pid == std::process::id() || popup.pid != info.pid {
                continue;
            }
            let Ok(popup_image) = crate::window_capture::capture_window(popup.hwnd) else {
                continue;
            };
            let popup_capture = if popup_image.own_content {
                "window"
            } else {
                "visible"
            };
            let rect = (
                popup_image.left,
                popup_image.top,
                popup_image.left.saturating_add(popup_image.width as i32),
                popup_image.top.saturating_add(popup_image.height as i32),
            );
            if rect.2 <= rect.0 || rect.3 <= rect.1 {
                continue;
            }
            if let Ok(shot) = self.capture_from_image(
                &popup_image,
                RegionCapture {
                    hwnd: popup.hwnd as i64,
                    region: rect,
                    anchor: (popup.left, popup.top, popup.right, popup.bottom),
                    scale,
                    role: "transient",
                    monitor: None,
                    z_index: 10 + index as i32,
                    capture: popup_capture,
                },
            ) {
                shots.push(shot);
            }
        }
        let primary = &shots[0];
        let legacy = json!({
            "id": primary["id"],
            "png_base64": primary["png_base64"],
            "scale": primary["scale"],
            "width": primary["width"],
            "height": primary["height"],
            "capture": primary["capture"],
            "note": format!(
                "region pixel (px,py) -> screen ({left}+px, {top}+py). Image pixels are region pixels times scale {scale:.3}; divide image pixels by scale before clicking. screenshotId binds one region and does not convert image pixels. Extra screenshots are per-monitor slices and owned popups. capture is {capture}: window means these pixels belong to the window even when it is covered.",
                left = primary["originX"].as_i64().unwrap_or(info.left as i64),
                top = primary["originY"].as_i64().unwrap_or(info.top as i64),
                scale = primary["scale"].as_f64().unwrap_or(1.0),
                capture = primary["capture"].as_str().unwrap_or(capture)
            ),
        });
        Ok((
            legacy,
            Value::Array(shots.iter().map(public_shot).collect()),
        ))
    }

    fn capture_from_image(
        &mut self,
        image: &crate::window_capture::WindowImage,
        mut shot: RegionCapture,
    ) -> Result<Value, String> {
        let (pixels, width, height, left, top) =
            crate::window_capture::crop_rgba(image, shot.region)?;
        shot.region = (
            left,
            top,
            left.saturating_add(width as i32),
            top.saturating_add(height as i32),
        );
        let (png, effective, out_w, out_h) = encode_rgba(&pixels, width, height, shot.scale)?;
        self.store_region(shot, png, effective, out_w, out_h)
    }

    fn capture_region(&mut self, shot: RegionCapture) -> Result<Value, String> {
        let rect = Rect::new(shot.region.0, shot.region.1, shot.region.2, shot.region.3);
        let (png, effective, width, height) = capture_png(rect, shot.scale)?;
        self.store_region(shot, png, effective, width, height)
    }

    fn store_region(
        &mut self,
        shot: RegionCapture,
        png: String,
        effective: f32,
        width: u32,
        height: u32,
    ) -> Result<Value, String> {
        self.shot_seq = self.shot_seq.saturating_add(1);
        let id = format!("ph-{}", self.shot_seq);
        let (virtual_left, virtual_top, virtual_width, virtual_height) = virtual_screen();
        let region = shot.region;
        self.shots.push(ShotRecord {
            id: id.clone(),
            hwnd: shot.hwnd,
            generation: self.generation,
            left: region.0,
            top: region.1,
            right: region.2,
            bottom: region.3,
            anchor_left: shot.anchor.0,
            anchor_top: shot.anchor.1,
            anchor_right: shot.anchor.2,
            anchor_bottom: shot.anchor.3,
            virtual_left,
            virtual_top,
            virtual_width,
            virtual_height,
        });
        if self.shots.len() > 32 {
            let extra = self.shots.len() - 32;
            self.shots.drain(0..extra);
        }
        Ok(json!({
            "id": id,
            "png_base64": png,
            "scale": effective,
            "width": width,
            "height": height,
            "originX": region.0,
            "originY": region.1,
            "zIndex": shot.z_index,
            "role": shot.role,
            "monitor": shot.monitor,
            "capture": shot.capture,
            "windowWidth": region.2.saturating_sub(region.0),
            "windowHeight": region.3.saturating_sub(region.1),
        }))
    }
}

struct WindowInfo {
    hwnd: isize,
    title: String,
    pid: u32,
    app: String,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl WindowInfo {
    fn to_json(&self) -> Value {
        json!({
            "id": self.hwnd as i64,
            "title": self.title,
            "pid": self.pid,
            "app": self.app,
            "rect": [self.left, self.top, self.right, self.bottom],
            "dpi": unsafe { GetDpiForWindow(HWND(self.hwnd as *mut std::ffi::c_void)) },
            "monitors": monitors_touching(self.left, self.top, self.right, self.bottom),
        })
    }
}

fn describe(hwnd: isize) -> Option<WindowInfo> {
    let handle = HWND(hwnd as *mut std::ffi::c_void);
    unsafe {
        if !IsWindowVisible(handle).as_bool() && hwnd == 0 {
            return None;
        }
        let length = GetWindowTextLengthW(handle);
        let mut buffer = vec![0u16; (length.max(0) as usize) + 1];
        let _ = GetWindowTextW(handle, &mut buffer);
        let title = u16_string(&buffer);
        let mut rect = windows::Win32::Foundation::RECT::default();
        let _ = GetWindowRect(handle, &mut rect);
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(handle, Some(&mut pid));
        if title.is_empty() && pid == 0 {
            return None;
        }
        Some(WindowInfo {
            hwnd,
            title,
            pid,
            app: process_image(pid),
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }
}

fn top_level_windows() -> Vec<isize> {
    let mut found = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_window),
            LPARAM(&mut found as *mut Vec<isize> as isize),
        );
    }
    found
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    if GetWindowTextLengthW(hwnd) == 0 {
        return BOOL(1);
    }
    let list = &mut *(lparam.0 as *mut Vec<isize>);
    list.push(hwnd.0 as isize);
    BOOL(1)
}

fn process_image(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        if ok.is_err() {
            return String::new();
        }
        u16_string(&buffer[..size as usize])
    }
}

fn process_image_of(hwnd: isize) -> String {
    describe(hwnd).map(|info| info.app).unwrap_or_default()
}

fn window_at(x: i32, y: i32) -> isize {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, WindowFromPoint, GA_ROOT};
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        let root = GetAncestor(hwnd, GA_ROOT);
        if root.0.is_null() {
            hwnd.0 as isize
        } else {
            root.0 as isize
        }
    }
}

pub(crate) fn focus(hwnd: isize) -> Result<(), String> {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetAncestor, ASFW_ANY, GA_ROOTOWNER,
    };
    let handle = HWND(hwnd as *mut std::ffi::c_void);
    let active_target = || unsafe {
        let active = GetForegroundWindow();
        active == handle || (!active.0.is_null() && GetAncestor(active, GA_ROOTOWNER) == handle)
    };
    for attempt in 0..3 {
        crate::check_running()?;
        // Preserve focus in an active target or its owned modal edit field.
        if active_target() {
            return Ok(());
        }
        unsafe {
            if IsIconic(handle).as_bool() {
                let _ = ShowWindow(handle, SW_RESTORE);
            }
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetForegroundWindow(handle);
            if !active_target() {
                let foreground = GetForegroundWindow();
                let foreground_thread = GetWindowThreadProcessId(foreground, None);
                let target_thread = GetWindowThreadProcessId(handle, None);
                let current = GetCurrentThreadId();
                let attached_foreground = !foreground.0.is_null()
                    && AttachThreadInput(current, foreground_thread, true).as_bool();
                let attached_target = AttachThreadInput(current, target_thread, true).as_bool();
                let _ = BringWindowToTop(handle);
                let _ = SetForegroundWindow(handle);
                if attached_target {
                    let _ = AttachThreadInput(current, target_thread, false);
                }
                if attached_foreground {
                    let _ = AttachThreadInput(current, foreground_thread, false);
                }
            }
        }
        if active_target() {
            return Ok(());
        }
        if attempt < 2 {
            crate::timing::sleep(Duration::from_millis(40))?;
        }
    }
    Err(format!("could not focus window {hwnd}; no input was sent"))
}

struct PointCovered {
    target: isize,
    x: i32,
    y: i32,
    covering: isize,
}

enum PointerBlock {
    Failed(String),
    Covered(PointCovered),
}

/// Bring `hwnd` forward. A point that a different window still covers is not an input target.
fn prepare_target(hwnd: isize, x: i32, y: i32) -> Result<(), PointerBlock> {
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    focus(hwnd).map_err(PointerBlock::Failed)?;
    if point_hits_target(hwnd, x, y) {
        return Ok(());
    }
    let handle = HWND(hwnd as *mut std::ffi::c_void);
    unsafe {
        let _ = SetWindowPos(
            handle,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = BringWindowToTop(handle);
        let _ = SetForegroundWindow(handle);
    }
    crate::timing::sleep(Duration::from_millis(40)).map_err(PointerBlock::Failed)?;
    if point_hits_target(hwnd, x, y) {
        Ok(())
    } else {
        Err(PointerBlock::Covered(PointCovered {
            target: hwnd,
            x,
            y,
            covering: window_at(x, y),
        }))
    }
}

impl Session {
    /// One raise, then a refusal. The refusal carries a fresh target screenshot when capture works.
    fn pointer_target(&mut self, hwnd: isize, x: i32, y: i32) -> Result<Option<Value>, String> {
        match prepare_target(hwnd, x, y) {
            Ok(()) => Ok(None),
            Err(PointerBlock::Failed(message)) => Err(message),
            Err(PointerBlock::Covered(miss)) => Ok(Some(covered_refusal(self, miss)?)),
        }
    }
}

fn covered_refusal(session: &mut Session, miss: PointCovered) -> Result<Value, String> {
    let covering = describe(miss.covering);
    let process = covering
        .as_ref()
        .map(|info| executable_name(&info.app))
        .unwrap_or_else(|| "unknown".into());
    let title = covering
        .as_ref()
        .map(|info| info.title.clone())
        .unwrap_or_default();
    let mut screenshot_id = None;
    let mut views = None;
    if let Some(info) = describe(miss.target) {
        match session.capture_window_views(&info, 0.5) {
            Ok(captured) => {
                screenshot_id = captured
                    .0
                    .get("id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                views = Some(captured);
            }
            Err(error) if error.contains("stopped by Escape") => return Err(error),
            Err(_) => {}
        }
    }
    let message = covered_message(
        miss.target,
        miss.x,
        miss.y,
        &process,
        &title,
        screenshot_id.as_deref(),
    );
    let mut body = json!({
        "acted": false,
        "reason": "covered",
        "retry": "once",
        "point": [miss.x, miss.y],
        "window_id": miss.target,
        "covering": {
            "hwnd": miss.covering,
            "title": title,
            "process": process,
        },
        "message": message,
    });
    if let Some((legacy, shots)) = views {
        body["screenshot"] = legacy;
        body["screenshots"] = shots;
    }
    Ok(body)
}

fn covered_message(
    target: isize,
    x: i32,
    y: i32,
    process: &str,
    title: &str,
    screenshot_id: Option<&str>,
) -> String {
    let who = if title.is_empty() {
        process.to_string()
    } else {
        format!("{process} ({title})")
    };
    let start = if process.eq_ignore_ascii_case("StartMenuExperienceHost.exe") {
        " The covering window is the Start menu."
    } else {
        ""
    };
    match screenshot_id {
        Some(id) => format!(
            "Point ({x}, {y}) is over {who}, not window {target}. No input was sent. The target was brought forward once.{start} Retry this input once with screenshotId {id}. x and y are physical pixels inside that new region; divide image pixels by scale. Do not click the covering window and do not reuse the previous screenshotId."
        ),
        None => format!(
            "Point ({x}, {y}) is over {who}, not window {target}. No input was sent. The target was brought forward once.{start} Call get_window_state for window {target}, then retry this input once with coordinates from that screenshot. Do not click the covering window and do not reuse the previous screenshotId."
        ),
    }
}

fn executable_name(path: &str) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .trim();
    if name.is_empty() {
        "unknown".into()
    } else {
        name.to_string()
    }
}

fn is_covered_refusal(value: &Value) -> bool {
    value.get("acted").and_then(|item| item.as_bool()) == Some(false)
        && value.get("reason").and_then(|item| item.as_str()) == Some("covered")
}

/// Batch responses keep the refusal and drop image bytes. The agent takes a new window shot.
fn batch_covered_result(value: &Value) -> Value {
    let target = value
        .get("window_id")
        .and_then(|item| item.as_i64())
        .unwrap_or(0);
    let point = value.get("point").and_then(|item| item.as_array());
    let x = point
        .and_then(|item| item.first())
        .and_then(|item| item.as_i64())
        .unwrap_or(0);
    let y = point
        .and_then(|item| item.get(1))
        .and_then(|item| item.as_i64())
        .unwrap_or(0);
    let process = value
        .get("covering")
        .and_then(|item| item.get("process"))
        .and_then(|item| item.as_str())
        .unwrap_or("unknown");
    let title = value
        .get("covering")
        .and_then(|item| item.get("title"))
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let mut compact = value.clone();
    if let Some(object) = compact.as_object_mut() {
        object.remove("screenshot");
        object.remove("screenshots");
        object.insert(
            "message".into(),
            json!(covered_message(
                target as isize,
                x as i32,
                y as i32,
                process,
                title,
                None
            )),
        );
    }
    compact
}

fn point_hits_target(target: isize, x: i32, y: i32) -> bool {
    let under = window_at(x, y);
    same_window_family(target, under, owner_of(target), owner_of(under))
}

fn owner_of(hwnd: isize) -> isize {
    if hwnd == 0 {
        return 0;
    }
    unsafe {
        let owner = GetAncestor(HWND(hwnd as *mut std::ffi::c_void), GA_ROOTOWNER);
        if owner.0.is_null() {
            hwnd
        } else {
            owner.0 as isize
        }
    }
}

fn same_window_family(
    target: isize,
    under: isize,
    target_owner: isize,
    under_owner: isize,
) -> bool {
    if under == 0 || target == 0 {
        return false;
    }
    under == target
        || under_owner == target
        || target_owner == under
        || (target_owner != 0 && under_owner == target_owner)
}

// The observation clock is separate from the physical idle gate. Mouse and
// keyboard hooks still own LAST_PHYSICAL; this only records that an action
// changed the screen.
static LAST_ACTION: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct StateSample {
    busy: bool,
    signature: u64,
}

fn note_action() {
    let mut slot = LAST_ACTION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = Some(Instant::now());
}

fn action_age_ms() -> Option<u64> {
    let slot = LAST_ACTION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.map(|at| at.elapsed().as_millis() as u64)
}

/// Sleep budget before the next observation. `elapsed_ms` is the age of the
/// last action. `samples` are the fingerprints read after the one-second floor,
/// in order. Two quiet samples with the same signature stop the wait. A visible
/// progress indicator or a changing signature keeps polling, up to five extra
/// seconds. The live loop in `Session::settle_before_observation` follows this
/// budget and uses a wall clock so a slow walk cannot run past it.
fn observation_wait_ms(elapsed_ms: u64, samples: &[StateSample]) -> u64 {
    if elapsed_ms >= STATE_FLOOR_MS + STATE_EXTRA_MS {
        return 0;
    }
    let mut waited = STATE_FLOOR_MS.saturating_sub(elapsed_ms);
    let mut extra_left = STATE_EXTRA_MS.saturating_sub(elapsed_ms.saturating_sub(STATE_FLOOR_MS));
    let mut previous: Option<StateSample> = None;
    for sample in samples {
        if let Some(prev) = previous {
            if !prev.busy && !sample.busy && prev.signature == sample.signature {
                return waited;
            }
        }
        if extra_left == 0 {
            return waited;
        }
        let slice = STATE_POLL_MS.min(extra_left);
        extra_left -= slice;
        waited += slice;
        previous = Some(*sample);
    }
    waited
}

fn range_is_busy(value: Option<f64>, maximum: Option<f64>) -> bool {
    match (value, maximum) {
        (Some(value), Some(maximum)) => value < maximum - 0.5,
        _ => true,
    }
}

fn class_looks_busy(class_name: &str) -> bool {
    let class = class_name.to_ascii_lowercase();
    class.contains("progress") || class.contains("busyindicator") || class.contains("spinner")
}

fn mix_signature(signature: u64, value: u64) -> u64 {
    signature.wrapping_mul(0x100000001b3).wrapping_add(value)
}

fn window_rect_signature(hwnd: isize) -> u64 {
    let mut rect = RECT::default();
    let ok = unsafe { GetWindowRect(HWND(hwnd as *mut std::ffi::c_void), &mut rect).is_ok() };
    if !ok {
        return 0;
    }
    let mut signature = 0u64;
    for value in [rect.left, rect.top, rect.right, rect.bottom] {
        signature = mix_signature(signature, value as u64);
    }
    signature
}

fn fold_state(
    walker: &UITreeWalker,
    element: &UIElement,
    depth: usize,
    signature: &mut u64,
    busy: &mut bool,
    count: &mut usize,
) {
    if *count >= STATE_WALK_NODES || depth > STATE_WALK_DEPTH {
        return;
    }
    *count += 1;
    let kind = element.get_control_type().ok();
    let offscreen = element.is_offscreen().unwrap_or(true);
    *signature = mix_signature(*signature, kind.map(|kind| kind as i32).unwrap_or(0) as u64);
    if let Ok(rect) = element.get_bounding_rectangle() {
        for value in [
            rect.get_left() / 16,
            rect.get_top() / 16,
            rect.get_right() / 16,
            rect.get_bottom() / 16,
        ] {
            *signature = mix_signature(*signature, value as u64);
        }
    }
    *signature = mix_signature(*signature, u64::from(offscreen));
    if !offscreen {
        *busy |= element_is_busy(element, kind);
    }
    if depth == STATE_WALK_DEPTH {
        return;
    }
    let mut child = walker.get_first_child(element).ok();
    let mut children = 0;
    while let Some(current) = child {
        if *count >= STATE_WALK_NODES || children >= MAX_CHILDREN {
            break;
        }
        fold_state(walker, &current, depth + 1, signature, busy, count);
        children += 1;
        child = walker.get_next_sibling(&current).ok();
    }
}

fn element_is_busy(element: &UIElement, kind: Option<ControlType>) -> bool {
    if kind == Some(ControlType::ScrollBar) {
        return false;
    }
    if kind == Some(ControlType::ProgressBar) {
        return progress_incomplete(element);
    }
    element
        .get_classname()
        .map(|name| class_looks_busy(&name))
        .unwrap_or(false)
}

fn progress_incomplete(element: &UIElement) -> bool {
    let Ok(pattern) = element.get_pattern::<UIRangeValuePattern>() else {
        return range_is_busy(None, None);
    };
    range_is_busy(pattern.get_value().ok(), pattern.get_maximum().ok())
}

fn note_injection() {
    note_action();
}

fn allow_list_on() -> bool {
    std::env::var("POINTHAND_ALLOW")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn ensure_token(image: &str) -> Result<(), String> {
    let raw = std::env::var("POINTHAND_ALLOW").unwrap_or_default();
    if app_allowed(image, &raw) {
        Ok(())
    } else {
        Err(format!("{image} is outside POINTHAND_ALLOW ({raw})"))
    }
}

fn app_allowed(image: &str, allow_csv: &str) -> bool {
    let allowed = allow_tokens(allow_csv);
    if allowed.is_empty() {
        return true;
    }
    let text = image.trim().to_ascii_lowercase().replace('/', "\\");
    let base = std::path::Path::new(&text)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let stem = std::path::Path::new(&base)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    allowed.contains(&text) || allowed.contains(&base) || allowed.contains(&stem)
}

fn allow_tokens(allow_csv: &str) -> std::collections::HashSet<String> {
    let mut tokens = std::collections::HashSet::new();
    for part in allow_csv.split(',') {
        let item = part.trim().to_ascii_lowercase().replace('/', "\\");
        if item.is_empty() {
            continue;
        }
        let base = std::path::Path::new(&item)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        let stem = std::path::Path::new(&base)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        if item.contains('\\') {
            tokens.insert(item);
        } else {
            tokens.insert(item);
            tokens.insert(base);
            tokens.insert(stem);
        }
    }
    tokens
}

fn u16_string(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

fn int_key(params: &Value, keys: &[&str]) -> Result<i32, String> {
    for key in keys {
        if let Some(value) = params.get(*key).and_then(|value| value.as_i64()) {
            return i32::try_from(value)
                .map_err(|_| format!("{key} is outside the signed 32-bit coordinate range"));
        }
    }
    Err(format!("missing or invalid {}", keys.join(" or ")))
}

const MAX_STROKE_SAMPLES: usize = 10_000;
fn densify(points: &[(i32, i32)], step: i32) -> Result<Vec<(i32, i32)>, String> {
    if step < 1 {
        return Err("stroke step must be positive".into());
    }
    let mut dense = Vec::new();
    if let Some(first) = points.first() {
        dense.push(*first);
    }
    for pair in points.windows(2) {
        let (x, y) = (i64::from(pair[0].0), i64::from(pair[0].1));
        let dx = i64::from(pair[1].0) - x;
        let dy = i64::from(pair[1].1) - y;
        let distance = dx.abs().max(dy.abs());
        let segments = ((distance + i64::from(step) - 1) / i64::from(step)).max(1);
        if segments > (MAX_STROKE_SAMPLES - dense.len()) as i64 {
            return Err("stroke exceeds 10000 samples; split it into smaller strokes".into());
        }
        for i in 1..=segments {
            dense.push((
                (x + dx * i / segments) as i32,
                (y + dy * i / segments) as i32,
            ));
        }
    }
    Ok(dense)
}

fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

fn to_absolute(x: i32, y: i32) -> (i32, i32) {
    absolute_for_bounds(x, y, virtual_screen())
}

fn absolute_for_bounds(x: i32, y: i32, bounds: (i32, i32, i32, i32)) -> (i32, i32) {
    let (left, top, width, height) = bounds;
    // Windows maps 65536 absolute bins onto the desktop. Aim at each pixel's
    // center, not its boundary, to avoid flooring into the preceding pixel.
    let axis = |value: i32, origin: i32, size: i32| {
        (((i64::from(value) - i64::from(origin)) * 65536 + 32768) / i64::from(size.max(1)))
            .clamp(0, 65535) as i32
    };
    (axis(x, left, width), axis(y, top, height))
}

pub(crate) fn move_pointer_pixel(x: i32, y: i32) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    crate::check_running()?;
    let (dx, dy) = to_absolute(x, y);
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE
                    | MOUSEEVENTF_ABSOLUTE
                    | MOUSEEVENTF_VIRTUALDESK
                    | MOUSEEVENTF_MOVE_NOCOALESCE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    if unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } != 1 {
        return Err("pointer input was blocked".into());
    }
    Ok(())
}

/// Extra time (ms) a stroke waits after pressing and before releasing the button.
/// Apps such as Illustrator read the cursor when they process an event, not when it
/// was sent, so a stroke needs a pause or its end point drifts. 0 keeps the fast path.
static STROKE_HOLD_MS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn stroke_hold() -> Result<(), String> {
    let ms = STROKE_HOLD_MS.load(std::sync::atomic::Ordering::Relaxed);
    if ms > 0 {
        crate::timing::sleep(Duration::from_millis(ms as u64))?;
    }
    Ok(())
}

fn stroke(points: &[(i32, i32)]) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
    };
    let place = MOUSEEVENTF_MOVE
        | MOUSEEVENTF_ABSOLUTE
        | MOUSEEVENTF_VIRTUALDESK
        | windows::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_MOVE_NOCOALESCE;
    let packet = |events: &[(
        windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
        i32,
        i32,
    )]|
     -> Result<(), String> {
        let inputs: Vec<INPUT> = events
            .iter()
            .map(|(flags, x, y)| INPUT {
                r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: *x,
                        dy: *y,
                        mouseData: 0,
                        dwFlags: *flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            })
            .collect();
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        note_injection();
        if sent != inputs.len() as u32 {
            return Err("mouse input was blocked".into());
        }
        Ok(())
    };
    // Button-down has to travel in the same packet as the first absolute
    // move. A separate relative LEFTDOWN lands at the previous cursor and
    // Paint records a stray line into the stroke.
    crate::overlay::signal(points[0].0, points[0].1, true);
    let (x, y) = to_absolute(points[0].0, points[0].1);
    crate::check_running()?;
    let outcome = (|| {
        packet(&[(place, x, y), (place | MOUSEEVENTF_LEFTDOWN, x, y)])?;
        stroke_hold()?;
        let mut overlay_tick = Instant::now();
        for (px, py) in points.iter().skip(1) {
            crate::check_running()?;
            if overlay_tick.elapsed() >= Duration::from_millis(8) {
                crate::overlay::signal(*px, *py, false);
                overlay_tick = Instant::now();
            }
            let (ax, ay) = to_absolute(*px, *py);
            packet(&[(place, ax, ay)])?;
            crate::timing::sleep(Duration::from_millis(4))?;
        }
        stroke_hold()?;
        Ok(())
    })();
    // Always release, including partial SendInput, cancellation and mid-stroke errors.
    let release = packet(&[(MOUSEEVENTF_LEFTUP, 0, 0)]);
    if let Some((x, y)) = points.last() {
        crate::overlay::signal(*x, *y, false);
    }
    outcome.and(release)
}

fn wheel_ticks(delta_px: i32) -> i32 {
    if delta_px == 0 {
        return 0;
    }
    let ticks = (delta_px as f32 / 120.0).round() as i32;
    if ticks == 0 {
        delta_px.signum()
    } else {
        ticks
    }
}

fn wheel(ticks: i32, horizontal: bool) -> Result<(), String> {
    if ticks == 0 {
        return Ok(());
    }
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    };
    let flags = if horizontal {
        MOUSEEVENTF_HWHEEL
    } else {
        MOUSEEVENTF_WHEEL
    };
    let input = INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: ticks.checked_mul(120).ok_or("scroll delta out of range")? as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        return Err("scroll input was blocked".into());
    }
    Ok(())
}

fn type_unicode(text: &str) -> Result<(), String> {
    for unit in text.encode_utf16() {
        crate::check_running()?;
        crate::overlay::touch();
        if unit == u16::from(b'\n') {
            tap_vk(0x0D)?;
            continue;
        }
        if unit == u16::from(b'\r') {
            continue;
        }
        send_unicode_unit(unit)?;
    }
    Ok(())
}

// Testable input transaction: every attempted key-down gets a best-effort
// release even if a later key is blocked or Escape interrupts the chord.
fn key_transaction(
    codes: &[u16],
    mut send: impl FnMut(u16, bool) -> Result<(), String>,
) -> Result<(), String> {
    let mut held = Vec::new();
    let outcome = (|| {
        for code in codes {
            held.push(*code);
            send(*code, false)?;
        }
        Ok(())
    })();
    let mut release_error = None;
    for code in held.into_iter().rev() {
        if let Err(error) = send(code, true) {
            if release_error.is_none() {
                release_error = Some(error);
            }
        }
    }
    outcome.and(match release_error {
        Some(error) => Err(error),
        None => Ok(()),
    })
}
fn send_combo(combo: &crate::keys::Combo) -> Result<(), String> {
    crate::check_running()?;
    crate::overlay::touch();
    let mut codes = combo.modifiers.clone();
    codes.push(combo.key);
    key_transaction(&codes, |code, up| {
        key_event(code, up)?;
        if !up {
            crate::timing::sleep(Duration::from_millis(35))?;
        }
        Ok(())
    })
}
fn tap_vk(code: u16) -> Result<(), String> {
    crate::check_running()?;
    key_transaction(&[code], key_event)
}

fn key_event(code: u16, up: bool) -> Result<(), String> {
    if !up {
        crate::check_running()?;
    }
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    let mut flags = windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0);
    if matches!(code, 0x21..=0x28 | 0x2C..=0x2E | 0x5B..=0x5C | 0x6F | 0x90) {
        flags |= windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(code),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        return Err("key input was blocked".into());
    }
    Ok(())
}

fn send_unicode_unit(unit: u16) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    let down = INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let up = INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32) };
    if sent != 2 {
        let _ = unsafe { SendInput(&[up], std::mem::size_of::<INPUT>() as i32) };
        return Err("text input was blocked".into());
    }
    Ok(())
}

/// Labels an agent can pass to `perform_secondary_action`. Scroll labels stop
/// at the end of that axis. A negative percent means the position is unknown.
fn action_labels(
    invoke: bool,
    expand: bool,
    collapse: bool,
    raise: bool,
    vertical: Option<f64>,
    horizontal: Option<f64>,
) -> Option<String> {
    let mut labels = Vec::new();
    if invoke {
        labels.push("Invoke");
    }
    if expand {
        labels.push("Expand");
    }
    if collapse {
        labels.push("Collapse");
    }
    if raise {
        labels.push("Raise");
    }
    if let Some(percent) = vertical {
        if percent != 0.0 {
            labels.push("Scroll Up");
        }
        if percent < 0.0 || percent < 100.0 {
            labels.push("Scroll Down");
        }
    }
    if let Some(percent) = horizontal {
        if percent != 0.0 {
            labels.push("Scroll Left");
        }
        if percent < 0.0 || percent < 100.0 {
            labels.push("Scroll Right");
        }
    }
    if labels.is_empty() {
        None
    } else {
        Some(labels.join(", "))
    }
}

fn element_action_labels(element: &UIElement) -> Option<String> {
    let mut expand = false;
    let mut collapse = false;
    if let Ok(pattern) = element.get_pattern::<UIExpandCollapsePattern>() {
        match pattern.get_state().ok() {
            Some(ExpandCollapseState::Collapsed) => expand = true,
            Some(ExpandCollapseState::Expanded) => collapse = true,
            Some(ExpandCollapseState::PartiallyExpanded) => {
                expand = true;
                collapse = true;
            }
            Some(ExpandCollapseState::LeafNode) | None => {}
        }
    }
    let scroll = element.get_pattern::<UIScrollPattern>().ok();
    let vertical = scroll.as_ref().and_then(|pattern| {
        pattern
            .is_vertically_scrollable()
            .ok()
            .filter(|scrollable| *scrollable)
            .map(|_| pattern.get_vertical_scroll_percent().unwrap_or(-1.0))
    });
    let horizontal = scroll.as_ref().and_then(|pattern| {
        pattern
            .is_horizontally_scrollable()
            .ok()
            .filter(|scrollable| *scrollable)
            .map(|_| pattern.get_horizontal_scroll_percent().unwrap_or(-1.0))
    });
    action_labels(
        element.get_pattern::<UIInvokePattern>().is_ok(),
        expand,
        collapse,
        element.get_pattern::<UIScrollItemPattern>().is_ok(),
        vertical,
        horizontal,
    )
}

fn canonical(action: &str) -> Result<String, String> {
    let key = action
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let mapped = match key.as_str() {
        "invoke" => "invoke",
        "expand" => "expand",
        "collapse" => "collapse",
        "raise" => "raise",
        "scroll up" => "scroll_up",
        "scroll down" => "scroll_down",
        "scroll left" => "scroll_left",
        "scroll right" => "scroll_right",
        _ => return Err(format!("unknown secondary action {action:?}")),
    };
    Ok(mapped.into())
}

fn scroll_pattern(pattern: &UIScrollPattern, action: &str) -> Result<(), String> {
    use uiautomation::types::ScrollAmount::{NoAmount, SmallDecrement, SmallIncrement};
    let kind = canonical(action)?;
    let (horizontal, vertical) = match kind.as_str() {
        "scroll_up" => (NoAmount, SmallDecrement),
        "scroll_down" => (NoAmount, SmallIncrement),
        "scroll_left" => (SmallDecrement, NoAmount),
        "scroll_right" => (SmallIncrement, NoAmount),
        _ => return Err(format!("not a scroll action: {action}")),
    };
    pattern
        .scroll(horizontal, vertical)
        .map_err(|err| err.to_string())
}

fn walk(
    walker: &UITreeWalker,
    element: &UIElement,
    depth: usize,
    nodes: &mut Vec<CachedNode>,
) -> Result<(), String> {
    if nodes.len() >= MAX_NODES || depth > MAX_DEPTH {
        return Ok(());
    }
    let index = nodes.len();
    let (line, focused, selected) = format_node(element, index, depth);
    nodes.push(CachedNode {
        element: element.clone(),
        line,
        focused,
        selected,
    });
    let mut child = walker.get_first_child(element).ok();
    let mut children = 0;
    while let Some(current) = child {
        if nodes.len() >= MAX_NODES || children >= MAX_CHILDREN {
            nodes[index].line.push_str(" children_truncated");
            break;
        }
        walk(walker, &current, depth + 1, nodes)?;
        children += 1;
        child = walker.get_next_sibling(&current).ok();
    }
    Ok(())
}

fn format_node(element: &UIElement, index: usize, depth: usize) -> (String, bool, bool) {
    let role = element
        .get_control_type()
        .map(|kind| format!("{kind:?}"))
        .unwrap_or_else(|_| "Unknown".into());
    let name = element
        .get_name()
        .unwrap_or_default()
        .replace(['\n', '"'], " ");
    let mut bits = vec![format!("[{index}]"), role];
    if !name.is_empty() {
        bits.push(format!("\"{name}\""));
    }
    if let Ok(pattern) = element.get_pattern::<UIValuePattern>() {
        if let Ok(value) = pattern.get_value() {
            if !value.is_empty() {
                let shown = value.chars().take(120).collect::<String>();
                bits.push(format!("value={shown:?}"));
            }
        }
        bits.push("editable".into());
    }
    let focused = element.has_keyboard_focus().unwrap_or(false);
    if focused {
        bits.push("focused".into());
    }
    let selected = element
        .get_pattern::<UISelectionItemPattern>()
        .ok()
        .and_then(|pattern| pattern.is_selected().ok())
        .unwrap_or(false);
    if selected {
        bits.push("selected".into());
    }
    if element.is_offscreen().unwrap_or(false) {
        bits.push("offscreen".into());
    }
    if !element.is_enabled().unwrap_or(true) {
        bits.push("disabled".into());
    }
    if let Some(actions) = element_action_labels(element) {
        bits.push(format!("actions={actions:?}"));
    }
    (
        format!("{:indent$}{}", "", bits.join(" "), indent = depth * 2),
        focused,
        selected,
    )
}

fn accessibility_payload(nodes: &[CachedNode], tree: &str) -> Value {
    let mut value = json!({ "tree": tree });
    if let Some(node) = nodes.iter().find(|node| node.focused) {
        value["focused_element"] = json!(node.line);
    }
    let selected: Vec<&str> = nodes
        .iter()
        .filter(|node| node.selected)
        .take(20)
        .map(|node| node.line.as_str())
        .collect();
    if !selected.is_empty() {
        value["selected_elements"] = json!(selected);
    }
    let (document, selection) = text_snapshot(nodes);
    if let Some(text) = document {
        value["document_text"] = json!(text);
    }
    if let Some(text) = selection {
        value["selected_text"] = json!(text);
    }
    value
}

fn text_snapshot(nodes: &[CachedNode]) -> (Option<String>, Option<String>) {
    let mut order = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.focused {
            order.push(index);
        }
    }
    if !nodes.is_empty() {
        order.push(0);
    }
    for index in 0..nodes.len().min(12) {
        order.push(index);
    }
    order.sort_unstable();
    order.dedup();
    let mut document = None;
    let mut selected = None;
    for index in order {
        let Ok(pattern) = nodes[index].element.get_pattern::<UITextPattern>() else {
            continue;
        };
        if document.is_none() {
            if let Ok(range) = pattern.get_document_range() {
                if let Ok(text) = range.get_text(8_000) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        document = Some(clip_text(trimmed, 8_000));
                    }
                }
            }
        }
        if selected.is_none() {
            if let Ok(ranges) = pattern.get_selection() {
                let mut parts = Vec::new();
                for range in ranges.iter().take(8) {
                    if let Ok(text) = range.get_text(2_000) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            parts.push(clip_text(trimmed, 2_000));
                        }
                    }
                }
                if !parts.is_empty() {
                    selected = Some(parts.join("\n"));
                }
            }
        }
    }
    (document, selected)
}

fn clip_text(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn screenshot_id_param(params: &Value) -> Result<Option<&str>, String> {
    match params
        .get("screenshotId")
        .or_else(|| params.get("screenshot_id"))
    {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.as_str())),
        Some(_) => Err("screenshotId must be a non-empty string".into()),
    }
}

fn check_shot(
    shots: &[ShotRecord],
    id: &str,
    window_id: Option<i64>,
    live: Option<(i32, i32, i32, i32)>,
) -> Result<i64, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("screenshotId must be a non-empty string".into());
    }
    let shot = shots
        .iter()
        .rev()
        .find(|shot| shot.id == id)
        .ok_or_else(|| format!("screenshotId {id} is not cached; call get_window_state again"))?;
    if let Some(window_id) = window_id {
        if window_id != shot.hwnd {
            return Err("screenshotId belongs to a different window".into());
        }
    }
    if shots
        .iter()
        .any(|item| item.hwnd == shot.hwnd && item.generation > shot.generation)
    {
        return Err("screenshotId is stale; call get_window_state again".into());
    }
    if let Some((left, top, right, bottom)) = live {
        if shot.anchor_left != left
            || shot.anchor_top != top
            || shot.anchor_right != right
            || shot.anchor_bottom != bottom
        {
            return Err(
                "window moved since screenshotId was captured; call get_window_state again".into(),
            );
        }
    }
    Ok(shot.hwnd)
}

fn point_in_frame(x: i32, y: i32, frame: RegionFrame) -> Result<(i32, i32, Option<i64>), String> {
    let (hwnd, left, top, right, bottom) = frame;
    let width = right.saturating_sub(left);
    let height = bottom.saturating_sub(top);
    if x < 0 || y < 0 || x >= width || y >= height {
        return Err("point leaves the screenshot region".into());
    }
    Ok((
        left.checked_add(x).ok_or("point x out of range")?,
        top.checked_add(y).ok_or("point y out of range")?,
        if hwnd == 0 { None } else { Some(hwnd) },
    ))
}

fn public_shot(shot: &Value) -> Value {
    let png = shot
        .get("png_base64")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    json!({
        "id": shot["id"],
        "url": format!("data:image/png;base64,{png}"),
        "width": shot["width"],
        "height": shot["height"],
        "originX": shot["originX"],
        "originY": shot["originY"],
        "zIndex": shot["zIndex"],
        "scale": shot["scale"],
        "role": shot["role"],
        "monitor": shot["monitor"],
        "windowWidth": shot["windowWidth"],
        "windowHeight": shot["windowHeight"],
        "capture": shot.get("capture").and_then(|value| value.as_str()).unwrap_or("visible"),
    })
}

fn intersection(
    window: (i32, i32, i32, i32),
    monitor: (i32, i32, i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    let left = window.0.max(monitor.0);
    let top = window.1.max(monitor.1);
    let right = window.2.min(monitor.2);
    let bottom = window.3.min(monitor.3);
    if right > left && bottom > top {
        Some((left, top, right, bottom))
    } else {
        None
    }
}

fn spanning_slices(
    window: (i32, i32, i32, i32),
    monitors: &[Monitor],
) -> Vec<(Monitor, (i32, i32, i32, i32))> {
    let hits: Vec<_> = monitors
        .iter()
        .filter_map(|monitor| {
            intersection(
                window,
                (monitor.left, monitor.top, monitor.right, monitor.bottom),
            )
            .map(|overlap| (monitor.clone(), overlap))
        })
        .collect();
    if hits.len() < 2 {
        Vec::new()
    } else {
        hits
    }
}

fn params_with_window<'a>(
    params: &'a Value,
    window_id: Option<i64>,
    stash: &'a mut Option<Value>,
) -> &'a Value {
    let Some(window_id) = window_id else {
        return params;
    };
    if params.get("window_id").and_then(|v| v.as_i64()).is_some() {
        return params;
    }
    let mut copy = params.clone();
    copy["window_id"] = json!(window_id);
    *stash = Some(copy);
    stash.as_ref().unwrap()
}

fn capture_png(rect: Rect, scale: f32) -> Result<(String, f32, u32, u32), String> {
    crate::guard::desktop_available()?;
    let shot = Screenshot::capture_rect(rect)
        .map_err(|err| err.to_string())?
        .to_rgba();
    encode_rgba(shot.pixels(), shot.width(), shot.height(), scale)
}

fn encode_rgba(
    pixels: &[u8],
    width: u32,
    height: u32,
    scale: f32,
) -> Result<(String, f32, u32, u32), String> {
    let scale = scale.clamp(0.05, 1.0);
    let longest = width.max(height).max(1);
    let effective = scale.min(1568.0 / longest as f32);
    let (pixels, out_w, out_h) = if effective < 0.999 {
        downsample(pixels, width, height, effective)
    } else {
        (pixels.to_vec(), width, height)
    };
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, out_w, out_h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(&pixels)
        .map_err(|err| err.to_string())?;
    drop(writer);
    Ok((
        base64::engine::general_purpose::STANDARD.encode(bytes),
        effective,
        out_w,
        out_h,
    ))
}

fn downsample(src: &[u8], width: u32, height: u32, scale: f32) -> (Vec<u8>, u32, u32) {
    let out_w = ((width as f32) * scale).round().max(1.0) as u32;
    let out_h = ((height as f32) * scale).round().max(1.0) as u32;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    for y in 0..out_h {
        let sy = ((y as f32) / scale).min(height.saturating_sub(1) as f32) as u32;
        for x in 0..out_w {
            let sx = ((x as f32) / scale).min(width.saturating_sub(1) as f32) as u32;
            let from = ((sy * width + sx) * 4) as usize;
            let to = ((y * out_w + x) * 4) as usize;
            if from + 4 <= src.len() && to + 4 <= out.len() {
                out[to..to + 4].copy_from_slice(&src[from..from + 4]);
            }
        }
    }
    (out, out_w, out_h)
}

fn paste_format(params: &Value) -> Result<&'static str, String> {
    match params.get("format").and_then(|value| value.as_str()) {
        None | Some("text") => Ok("text"),
        Some("md") | Some("markdown") => Ok("md"),
        Some("html") => Ok("html"),
        Some(_) => Err("paste format must be text, md, or html".into()),
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn safe_href(url: &str) -> Option<&str> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let allowed = lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:");
    if !allowed
        || url
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '<' | '>'))
    {
        return None;
    }
    Some(url)
}

fn inline_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('`') {
            if let Some(end) = inner.find('`') {
                out.push_str("<code>");
                out.push_str(&escape_html(&inner[..end]));
                out.push_str("</code>");
                rest = &inner[end + 1..];
                continue;
            }
        }
        if let Some(inner) = rest.strip_prefix("**") {
            if let Some(end) = inner.find("**") {
                out.push_str("<strong>");
                out.push_str(&escape_html(&inner[..end]));
                out.push_str("</strong>");
                rest = &inner[end + 2..];
                continue;
            }
        }
        if let Some((inner, next)) = take_emphasis(rest, '*') {
            out.push_str("<em>");
            out.push_str(&escape_html(inner));
            out.push_str("</em>");
            rest = next;
            continue;
        }
        if let Some((inner, next)) = take_emphasis(rest, '_') {
            out.push_str("<em>");
            out.push_str(&escape_html(inner));
            out.push_str("</em>");
            rest = next;
            continue;
        }
        if let Some((label, url, used)) = parse_link(rest) {
            if let Some(href) = safe_href(url) {
                out.push_str("<a href=\"");
                out.push_str(&escape_html(href));
                out.push_str("\">");
                out.push_str(&inline_markdown(label));
                out.push_str("</a>");
            } else {
                out.push_str(&inline_markdown(label));
            }
            rest = &rest[used..];
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\u{0}');
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
        rest = &rest[ch.len_utf8()..];
    }
    out
}

fn take_emphasis(text: &str, marker: char) -> Option<(&str, &str)> {
    if !text.starts_with(marker) || text.chars().nth(1) == Some(marker) {
        return None;
    }
    let inner = &text[marker.len_utf8()..];
    let end = inner.find(marker)?;
    if end == 0 || inner[end..].chars().nth(1) == Some(marker) {
        return None;
    }
    Some((&inner[..end], &inner[end + marker.len_utf8()..]))
}

fn parse_link(text: &str) -> Option<(&str, &str, usize)> {
    if !text.starts_with('[') {
        return None;
    }
    let label_end = text.find(']')?;
    if !text[label_end + 1..].starts_with('(') {
        return None;
    }
    let url_start = label_end + 2;
    let url_end = text[url_start..].find(')')?;
    Some((
        &text[1..label_end],
        &text[url_start..url_start + url_end],
        url_start + url_end + 1,
    ))
}

fn heading_level(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = line.get(hashes..)?;
    let text = rest.strip_prefix(' ')?.trim();
    if text.is_empty() {
        return None;
    }
    Some((hashes, text))
}

fn unordered_item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn ordered_item(line: &str) -> Option<&str> {
    let digits = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits == 0 || digits > 6 {
        return None;
    }
    line[digits..]
        .strip_prefix(". ")
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn markdown_to_html(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut html = String::new();
    let mut paragraph = String::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if line.is_empty() {
            flush_paragraph(&mut html, &mut paragraph);
            index += 1;
            continue;
        }
        if line.starts_with("```") {
            flush_paragraph(&mut html, &mut paragraph);
            index += 1;
            let mut code = String::new();
            while index < lines.len() && !lines[index].trim_start().starts_with("```") {
                code.push_str(lines[index]);
                code.push('\n');
                index += 1;
            }
            if index < lines.len() {
                index += 1;
            }
            html.push_str("<pre><code>");
            html.push_str(&escape_html(&code));
            html.push_str("</code></pre>");
            continue;
        }
        if let Some((level, text)) = heading_level(line) {
            flush_paragraph(&mut html, &mut paragraph);
            html.push_str(&format!("<h{level}>{}</h{level}>", inline_markdown(text)));
            index += 1;
            continue;
        }
        if unordered_item(line).is_some() {
            flush_paragraph(&mut html, &mut paragraph);
            html.push_str("<ul>");
            while index < lines.len() {
                let Some(item) = lines
                    .get(index)
                    .and_then(|line| unordered_item(line.trim()))
                else {
                    break;
                };
                html.push_str("<li>");
                html.push_str(&inline_markdown(item));
                html.push_str("</li>");
                index += 1;
            }
            html.push_str("</ul>");
            continue;
        }
        if ordered_item(line).is_some() {
            flush_paragraph(&mut html, &mut paragraph);
            html.push_str("<ol>");
            while index < lines.len() {
                let Some(item) = lines.get(index).and_then(|line| ordered_item(line.trim())) else {
                    break;
                };
                html.push_str("<li>");
                html.push_str(&inline_markdown(item));
                html.push_str("</li>");
                index += 1;
            }
            html.push_str("</ol>");
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(line);
        index += 1;
    }
    flush_paragraph(&mut html, &mut paragraph);
    html
}

fn flush_paragraph(html: &mut String, paragraph: &mut String) {
    let trimmed = paragraph.trim();
    if !trimmed.is_empty() {
        html.push_str("<p>");
        html.push_str(&inline_markdown(trimmed));
        html.push_str("</p>");
    }
    paragraph.clear();
}

fn html_to_plain(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            let Some(end) = stripped.find('>') else {
                break;
            };
            let name = tag_name(&stripped[..end]);
            if name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style") {
                let close = format!("</{name}");
                let search = &stripped[end + 1..];
                if let Some(pos) = search
                    .to_ascii_lowercase()
                    .find(&close.to_ascii_lowercase())
                {
                    let after = &search[pos + close.len()..];
                    rest = after.find('>').map(|at| &after[at + 1..]).unwrap_or("");
                    continue;
                }
            }
            if is_block_tag(&name) && !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            rest = &stripped[end + 1..];
            continue;
        }
        if let Some((ch, used)) = html_entity(rest) {
            out.push(ch);
            rest = &rest[used..];
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\u{0}');
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tag_name(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string()
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "p" | "div"
            | "br"
            | "li"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "blockquote"
            | "ul"
            | "ol"
    )
}

fn html_entity(text: &str) -> Option<(char, usize)> {
    let body = text.strip_prefix('&')?;
    let end = body.find(';')?;
    let name = &body[..end];
    let ch = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" | "#39" => '\'',
        "nbsp" => '\u{00a0}',
        _ if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) => {
            char::from_u32(u32::from_str_radix(hex, 16).ok()?)?
        }
        _ if let Some(dec) = name.strip_prefix('#') => char::from_u32(dec.parse().ok()?)?,
        _ => return None,
    };
    Some((ch, end + 2))
}

/// Windows HTML clipboard bytes. Offsets are UTF-8 byte positions of the fragment.
fn html_clipboard_document(fragment: &str) -> Vec<u8> {
    const PREFIX: &str = "<html>\r\n<body>\r\n<!--StartFragment-->";
    const SUFFIX: &str = "<!--EndFragment-->\r\n</body>\r\n</html>";
    const HEADER: &str = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
    let start_html = HEADER.len();
    let start_fragment = start_html + PREFIX.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + SUFFIX.len();
    let mut bytes = format!(
        "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
    )
    .into_bytes();
    debug_assert_eq!(bytes.len(), HEADER.len());
    bytes.extend_from_slice(PREFIX.as_bytes());
    bytes.extend_from_slice(fragment.as_bytes());
    bytes.extend_from_slice(SUFFIX.as_bytes());
    bytes.push(0);
    bytes
}

fn html_format_id() -> Result<u32, String> {
    use std::sync::OnceLock;
    use windows::core::PCWSTR;
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    static FORMAT: OnceLock<u32> = OnceLock::new();
    if let Some(id) = FORMAT.get() {
        return Ok(*id);
    }
    let name: Vec<u16> = "HTML Format"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let id = unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) };
    if id == 0 {
        return Err("could not register the HTML clipboard format".into());
    }
    let _ = FORMAT.set(id);
    Ok(id)
}

fn global_copy(bytes: &[u8]) -> Result<windows::Win32::Foundation::HGLOBAL, String> {
    use windows::Win32::Foundation::GlobalFree;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|err| err.to_string())?;
        let locked = GlobalLock(handle);
        if locked.is_null() {
            let _ = GlobalFree(Some(handle));
            return Err("could not lock clipboard memory".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), locked.cast(), bytes.len());
        let _ = GlobalUnlock(handle);
        Ok(handle)
    }
}

fn write_rich_clipboard(plain: &str, html: &str) -> Result<(), String> {
    use windows::Win32::Foundation::GlobalFree;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    let mut wide = Vec::new();
    for unit in plain.encode_utf16().chain(std::iter::once(0)) {
        wide.extend_from_slice(&unit.to_ne_bytes());
    }
    let html_bytes = html_clipboard_document(html);
    let format = html_format_id()?;
    unsafe {
        let text_handle = global_copy(&wide)?;
        let html_handle = match global_copy(&html_bytes) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = GlobalFree(Some(text_handle));
                return Err(error);
            }
        };
        if let Err(error) = OpenClipboard(None) {
            let _ = GlobalFree(Some(text_handle));
            let _ = GlobalFree(Some(html_handle));
            return Err(error.to_string());
        }
        let _ = EmptyClipboard();
        if SetClipboardData(13u32, Some(as_handle(text_handle))).is_err() {
            let _ = GlobalFree(Some(text_handle));
            let _ = GlobalFree(Some(html_handle));
            let _ = CloseClipboard();
            return Err("could not set clipboard text".into());
        }
        if SetClipboardData(format, Some(as_handle(html_handle))).is_err() {
            let _ = GlobalFree(Some(html_handle));
            let _ = CloseClipboard();
            return Err("could not set clipboard html".into());
        }
        let _ = CloseClipboard();
    }
    Ok(())
}

fn read_clipboard() -> Result<String, String> {
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    unsafe {
        OpenClipboard(None).map_err(|err| err.to_string())?;
        let handle = GetClipboardData(13u32).map_err(|err| {
            let _ = CloseClipboard();
            err.to_string()
        })?;
        if handle.0.is_null() {
            let _ = CloseClipboard();
            return Ok(String::new());
        }
        let memory = windows::Win32::Foundation::HGLOBAL(handle.0);
        let locked = GlobalLock(memory);
        if locked.is_null() {
            let _ = CloseClipboard();
            return Err("could not read the clipboard".into());
        }
        let mut length = 0usize;
        while length < 1_048_576 && *(locked as *const u16).add(length) != 0 {
            length += 1;
        }
        let text =
            String::from_utf16_lossy(std::slice::from_raw_parts(locked as *const u16, length));
        let _ = GlobalUnlock(memory);
        let _ = CloseClipboard();
        Ok(text.trim_end_matches('\u{0}').to_string())
    }
}

fn write_clipboard(text: &str) -> Result<(), String> {
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * 2;
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|err| err.to_string())?;
        let locked = GlobalLock(handle);
        if locked.is_null() {
            return Err("could not lock clipboard memory".into());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), locked as *mut u16, wide.len());
        let _ = GlobalUnlock(handle);
        OpenClipboard(None).map_err(|err| err.to_string())?;
        let _ = EmptyClipboard();
        if SetClipboardData(13u32, Some(as_handle(handle))).is_err() {
            let _ = CloseClipboard();
            return Err("could not set clipboard text".into());
        }
        let _ = CloseClipboard();
    }
    Ok(())
}

/// SetClipboardData wants a HANDLE. GlobalAlloc returns HGLOBAL, which is that handle.
fn as_handle(handle: windows::Win32::Foundation::HGLOBAL) -> windows::Win32::Foundation::HANDLE {
    windows::Win32::Foundation::HANDLE(handle.0)
}

fn start_menu(query: &str) -> Vec<Value> {
    let needle = query.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut roots = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(programdata) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(programdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    for root in roots {
        let _ = visit_shortcuts(&root, &needle, &mut found);
        if found.len() >= 200 {
            break;
        }
    }
    found
}

fn visit_shortcuts(dir: &PathBuf, needle: &str, found: &mut Vec<Value>) -> std::io::Result<()> {
    if found.len() >= 200 || !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_shortcuts(&path, needle, found)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".lnk") {
            continue;
        }
        let display = name.trim_end_matches(".lnk").trim_end_matches(".LNK");
        let full = path.display().to_string();
        if !needle.is_empty()
            && !display.to_ascii_lowercase().contains(needle)
            && !full.to_ascii_lowercase().contains(needle)
        {
            continue;
        }
        found.push(json!({
            "id": full,
            "displayName": display,
            "isRunning": false,
            "windows": [],
        }));
        if found.len() >= 200 {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod policy_tests {
    use super::app_allowed;

    #[test]
    fn accessibility_lines_name_only_the_actions_that_can_run() {
        assert_eq!(
            super::action_labels(true, true, false, true, Some(0.0), Some(40.0)).as_deref(),
            Some("Invoke, Expand, Raise, Scroll Down, Scroll Left, Scroll Right")
        );
        assert_eq!(
            super::action_labels(false, false, true, false, Some(100.0), Some(0.0)).as_deref(),
            Some("Collapse, Scroll Up, Scroll Right")
        );
        assert_eq!(
            super::action_labels(false, false, false, false, Some(-1.0), None).as_deref(),
            Some("Scroll Up, Scroll Down")
        );
        assert_eq!(
            super::action_labels(false, false, false, false, None, None),
            None
        );
    }

    #[test]
    fn observation_waits_for_a_quiet_screen_and_stops_at_six_seconds() {
        let quiet = super::StateSample {
            busy: false,
            signature: 7,
        };
        assert_eq!(super::observation_wait_ms(0, &[quiet, quiet]), 1200);
        assert_eq!(
            super::observation_wait_ms(
                6000,
                &[super::StateSample {
                    busy: true,
                    signature: 1
                }]
            ),
            0
        );
        let busy = super::StateSample {
            busy: true,
            signature: 1,
        };
        assert_eq!(super::observation_wait_ms(0, &[busy; 30]), 6000);
        assert_eq!(
            super::observation_wait_ms(
                0,
                &[
                    super::StateSample {
                        busy: false,
                        signature: 1
                    },
                    super::StateSample {
                        busy: false,
                        signature: 2
                    },
                    super::StateSample {
                        busy: false,
                        signature: 2
                    },
                ]
            ),
            1400
        );
        let settled = super::StateSample {
            busy: false,
            signature: 3,
        };
        assert_eq!(super::observation_wait_ms(2500, &[settled, settled]), 200);
        assert!(!super::range_is_busy(Some(100.0), Some(100.0)));
        assert!(super::range_is_busy(Some(40.0), Some(100.0)));
        assert!(super::range_is_busy(None, None));
        assert!(super::class_looks_busy("msctls_progress32"));
        assert!(super::class_looks_busy("ProgressRing"));
        assert!(super::class_looks_busy("BusyIndicator"));
        assert!(!super::class_looks_busy("ScrollBar"));
    }

    #[test]
    fn a_covered_point_belongs_to_the_same_window_family() {
        assert!(super::same_window_family(10, 10, 10, 10));
        assert!(super::same_window_family(10, 22, 10, 10));
        assert!(super::same_window_family(10, 9, 9, 9));
        assert!(!super::same_window_family(10, 40, 10, 40));
        assert!(!super::same_window_family(10, 0, 10, 0));
    }

    #[test]
    fn a_covered_point_names_the_window_in_front_and_asks_for_one_retry() {
        let with_shot = super::covered_message(
            10,
            4,
            5,
            "StartMenuExperienceHost.exe",
            "Start",
            Some("ph-3"),
        );
        assert!(with_shot.contains("StartMenuExperienceHost.exe"));
        assert!(with_shot.contains("Start menu"));
        assert!(with_shot.contains("ph-3"));
        assert!(with_shot.contains("once"));
        assert!(with_shot.contains("No input was sent"));
        let fresh = super::covered_message(10, 4, 5, "notepad.exe", "Untitled", None);
        assert!(fresh.contains("get_window_state"));
        assert!(!fresh.contains("Start menu"));
        assert_eq!(
            super::executable_name(r"C:\Windows\System32\StartMenuExperienceHost.exe"),
            "StartMenuExperienceHost.exe"
        );
        assert_eq!(super::executable_name(""), "unknown");
        let refusal = serde_json::json!({
            "acted": false,
            "reason": "covered",
            "point": [4, 5],
            "window_id": 10,
            "covering": {"hwnd": 40, "title": "Start", "process": "StartMenuExperienceHost.exe"},
            "message": with_shot,
            "screenshot": {"id": "ph-3", "png_base64": "QUJD"},
            "screenshots": [{"id": "ph-3", "url": "data:image/png;base64,QUJD"}]
        });
        assert!(super::is_covered_refusal(&refusal));
        let compact = super::batch_covered_result(&refusal);
        assert!(compact.get("screenshot").is_none());
        assert!(compact.get("screenshots").is_none());
        assert!(compact["message"]
            .as_str()
            .unwrap()
            .contains("get_window_state"));
        assert!(!compact["message"].as_str().unwrap().contains("ph-3"));
    }

    #[test]
    fn paste_formats_build_rich_clipboard_bytes_without_touching_the_clipboard() {
        assert_eq!(super::paste_format(&serde_json::json!({})).unwrap(), "text");
        assert_eq!(
            super::paste_format(&serde_json::json!({"format": "markdown"})).unwrap(),
            "md"
        );
        assert!(super::paste_format(&serde_json::json!({"format": "rtf"})).is_err());
        let bytes = super::html_clipboard_document("<b>ç</b>");
        let text = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
        let start = header_offset(text, "StartFragment:");
        let end = header_offset(text, "EndFragment:");
        assert_eq!(&text.as_bytes()[start..end], "<b>ç</b>".as_bytes());
        let html = super::markdown_to_html(
            "# Title\n\n**bold** and `x < y`\n\n- one\n\n[ok](https://example.com)\n[bad](javascript:alert(1))\n\n```\n<tag>\n```\n",
        );
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<code>x &lt; y</code>"));
        assert!(html.contains("<li>one</li>"));
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("<pre><code>&lt;tag&gt;\n</code></pre>"));
        assert!(!html.to_ascii_lowercase().contains("javascript:"));
        let plain = super::html_to_plain("<p>Hello<br>there</p><script>bad()</script>");
        assert!(plain.contains("Hello"));
        assert!(plain.contains("there"));
        assert!(!plain.contains("bad"));
        assert!(!plain.contains("<p>"));
    }

    fn header_offset(text: &str, key: &str) -> usize {
        let line = text.lines().find(|line| line.starts_with(key)).unwrap();
        line[key.len()..].trim().parse().unwrap()
    }

    #[test]
    fn densify_keeps_endpoints_and_step() {
        let dense = super::densify(&[(0, 0), (20, 0)], 6).unwrap();
        assert_eq!(dense.first(), Some(&(0, 0)));
        assert_eq!(dense.last(), Some(&(20, 0)));
        assert!(dense
            .windows(2)
            .all(|pair| (pair[1].0 - pair[0].0).abs() <= 6));
    }

    #[test]
    fn screenshot_id_accepts_the_latest_shot_for_that_window() {
        let shots = vec![
            shot("ph-1", 7, 10, 1),
            shot("ph-2", 8, 40, 1),
            shot("ph-3", 7, 10, 2),
        ];
        assert_eq!(
            super::check_shot(&shots, " ph-3 ", Some(7), Some((10, 0, 110, 50))).unwrap(),
            7
        );
        assert_eq!(super::check_shot(&shots, "ph-2", None, None).unwrap(), 8);
    }

    #[test]
    fn screenshot_id_rejects_unknown_stale_foreign_and_moved() {
        let shots = vec![shot("ph-1", 7, 10, 1), shot("ph-2", 7, 12, 2)];
        assert!(super::check_shot(&shots, "missing", None, None)
            .unwrap_err()
            .contains("not cached"));
        assert!(super::check_shot(&shots, "ph-1", Some(7), None)
            .unwrap_err()
            .contains("stale"));
        assert!(super::check_shot(&shots, "ph-2", Some(9), None)
            .unwrap_err()
            .contains("different"));
        assert!(
            super::check_shot(&shots, "ph-2", Some(7), Some((10, 0, 110, 50)))
                .unwrap_err()
                .contains("moved")
        );
        assert!(super::check_shot(&shots, "  ", None, None).is_err());
        assert!(super::screenshot_id_param(&serde_json::json!({"screenshotId": 4})).is_err());
    }

    fn shot(id: &str, hwnd: i64, left: i32, generation: u64) -> super::ShotRecord {
        super::ShotRecord {
            id: id.into(),
            hwnd,
            generation,
            left,
            top: 0,
            right: left + 100,
            bottom: 50,
            anchor_left: left,
            anchor_top: 0,
            anchor_right: left + 100,
            anchor_bottom: 50,
            virtual_left: 0,
            virtual_top: 0,
            virtual_width: 1920,
            virtual_height: 1080,
        }
    }

    #[test]
    fn sibling_regions_stay_valid_until_the_next_capture() {
        let mut full = shot("ph-1", 7, 10, 2);
        full.right = 140;
        full.anchor_right = 140;
        let mut slice = shot("ph-2", 7, 100, 2);
        slice.left = 100;
        slice.top = 0;
        slice.right = 140;
        slice.bottom = 50;
        slice.anchor_left = 10;
        slice.anchor_right = 140;
        let shots = vec![full, slice];
        assert_eq!(
            super::point_in_frame(10, 20, (0, -1920, 0, -1000, 1080)).unwrap(),
            (-1910, 20, None)
        );
        assert_eq!(
            super::check_shot(&shots, "ph-1", Some(7), Some((10, 0, 140, 50))).unwrap(),
            7
        );
        assert!(super::check_shot(&shots, "ph-2", Some(7), Some((10, 0, 140, 50))).is_ok());
        let replaced = vec![shot("ph-1", 7, 10, 2), shot("ph-4", 7, 10, 4)];
        assert!(super::check_shot(&replaced, "ph-1", Some(7), None)
            .unwrap_err()
            .contains("stale"));
    }

    #[test]
    fn a_window_on_two_monitors_splits_into_both_regions() {
        let monitors = vec![
            monitor_at("left", false, -1920, 0, 0, 1080),
            monitor_at("right", true, 0, 0, 1920, 1080),
        ];
        let slices = super::spanning_slices((-100, 40, 80, 200), &monitors);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].1, (-100, 40, 0, 200));
        assert_eq!(slices[1].1, (0, 40, 80, 200));
        assert!(super::spanning_slices((10, 10, 20, 20), &monitors).is_empty());
    }

    fn monitor_at(
        device: &str,
        primary: bool,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) -> super::Monitor {
        super::Monitor {
            device: device.into(),
            primary,
            left,
            top,
            right,
            bottom,
            work_left: left,
            work_top: top,
            work_right: right,
            work_bottom: bottom,
            dpi_x: 96,
            dpi_y: 96,
        }
    }

    #[test]
    fn allow_list_matches_path_name_or_stem() {
        assert!(app_allowed(r"C:\Windows\notepad.exe", ""));
        let allow = r"C:\Windows\System32\notepad.exe, calc, notepad.exe";
        assert!(app_allowed(r"C:\Windows\System32\notepad.exe", allow));
        assert!(app_allowed("notepad.exe", allow));
        assert!(app_allowed("NOTEPAD", allow));
        assert!(app_allowed("calc.exe", allow));
        assert!(!app_allowed(r"C:\Windows\explorer.exe", allow));
    }
}

#[cfg(test)]
mod input_transaction_tests {
    use super::*;
    #[test]
    fn releases_every_key_after_partial_failure() {
        let mut events = Vec::new();
        let result = key_transaction(&[0x11, 0x10, 0x41], |code, up| {
            events.push((code, up));
            if code == 0x41 && !up {
                Err("blocked".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(
            events,
            [
                (0x11, false),
                (0x10, false),
                (0x41, false),
                (0x41, true),
                (0x10, true),
                (0x11, true)
            ]
        );
    }
    #[test]
    fn attempts_remaining_releases_when_one_release_fails() {
        let mut events = Vec::new();
        let result = key_transaction(&[0x11, 0x41], |code, up| {
            events.push((code, up));
            if code == 0x41 && up {
                Err("blocked".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(events.last(), Some(&(0x11, true)));
    }
    #[test]
    fn huge_paths_are_rejected_without_overflow() {
        assert!(densify(&[(i32::MIN, i32::MIN), (i32::MAX, i32::MAX)], 1).is_err());
        assert!(densify(&[(0, 0), (1, 1)], 0).is_err());
    }
    #[test]
    fn coordinates_do_not_wrap() {
        assert!(int_key(&json!({"x":i64::MAX}), &["x"]).is_err());
    }
}

#[cfg(test)]
mod release_security_tests {
    use super::*;
    #[test]
    fn explicit_allow_paths_do_not_whitelist_namesakes() {
        let allow = r"C:\Windows\System32\notepad.exe";
        assert!(app_allowed(r"C:\Windows\System32\notepad.exe", allow));
        assert!(!app_allowed(r"C:\Unapproved\notepad.exe", allow));
        assert!(!app_allowed("notepad.exe", allow));
        assert!(app_allowed(r"C:\Unapproved\notepad.exe", "notepad"));
    }
    #[test]
    fn absolute_coordinates_round_trip_to_pixel_centers_on_negative_desktops() {
        let bounds = (-1920, -1080, 4480, 2520);
        for x in [-1920, -1919, -1700, -1, 0, 1919, 2559] {
            for y in [-1080, -1079, -1, 0, 1079, 1439] {
                let (ax, ay) = absolute_for_bounds(x, y, bounds);
                assert_eq!(
                    i64::from(ax) * i64::from(bounds.2) / 65536 + i64::from(bounds.0),
                    i64::from(x)
                );
                assert_eq!(
                    i64::from(ay) * i64::from(bounds.3) / 65536 + i64::from(bounds.1),
                    i64::from(y)
                );
            }
        }
        assert_eq!(absolute_for_bounds(i32::MIN, i32::MAX, bounds), (0, 65535));
    }
}

fn instant_motion(params: &Value) -> Result<bool, String> {
    match params.get("motion").and_then(Value::as_str) {
        None | Some("natural") => Ok(false),
        Some("instant") => Ok(true),
        _ => Err("motion must be natural or instant".into()),
    }
}

pub fn system_info() -> Result<Value, String> {
    let monitors = monitors()?;
    let (left, top, width, height) = virtual_screen();
    Ok(json!({
        "platform": "windows",
        "arch": std::env::consts::ARCH,
        "coordinate_space": "physical_pixels",
        "monitors": monitors.iter().map(monitor_json).collect::<Vec<_>>(),
        "virtual_screen": { "left": left, "top": top, "width": width, "height": height }
    }))
}

fn monitors() -> Result<Vec<Monitor>, String> {
    let mut found = Vec::new();
    unsafe {
        if !EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor),
            LPARAM(&mut found as *mut Vec<Monitor> as isize),
        )
        .as_bool()
        {
            return Err("cannot enumerate display monitors".into());
        }
    }
    Ok(found)
}

fn monitor_json(monitor: &Monitor) -> Value {
    let dpi = if monitor.dpi_x == 0 {
        Value::Null
    } else {
        json!({
            "x": monitor.dpi_x,
            "y": monitor.dpi_y,
            "scale_percent": f64::from(monitor.dpi_x) * 100.0 / 96.0
        })
    };
    json!({
        "device": monitor.device,
        "primary": monitor.primary,
        "rect": [monitor.left, monitor.top, monitor.right, monitor.bottom],
        "work_rect": [monitor.work_left, monitor.work_top, monitor.work_right, monitor.work_bottom],
        "dpi": dpi,
    })
}

fn monitors_touching(left: i32, top: i32, right: i32, bottom: i32) -> Value {
    let Ok(list) = monitors() else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        list.iter()
            .filter_map(|monitor| {
                let overlap = intersection(
                    (left, top, right, bottom),
                    (monitor.left, monitor.top, monitor.right, monitor.bottom),
                )?;
                let mut row = monitor_json(monitor);
                if let Some(map) = row.as_object_mut() {
                    map.insert(
                        "overlap".into(),
                        json!([overlap.0, overlap.1, overlap.2, overlap.3]),
                    );
                }
                Some(row)
            })
            .collect(),
    )
}

struct TransientSearch {
    root: isize,
    found: Vec<isize>,
}

fn transient_windows(root: isize) -> Vec<isize> {
    let mut search = TransientSearch {
        root,
        found: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_transient),
            LPARAM(&mut search as *mut TransientSearch as isize),
        );
    }
    search.found
}

unsafe extern "system" fn enum_transient(hwnd: HWND, data: LPARAM) -> BOOL {
    let search = &mut *(data.0 as *mut TransientSearch);
    if search.found.len() >= 4 || hwnd.0 as isize == search.root {
        return BOOL(1);
    }
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return BOOL(1);
    }
    let owner = GetAncestor(hwnd, GA_ROOTOWNER);
    if owner.0.is_null() || owner.0 as isize != search.root {
        return BOOL(1);
    }
    search.found.push(hwnd.0 as isize);
    BOOL(1)
}

unsafe extern "system" fn enum_monitor(
    monitor: HMONITOR,
    _: HDC,
    _: *mut windows::Win32::Foundation::RECT,
    data: LPARAM,
) -> BOOL {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
        return BOOL(1);
    }
    let (mut x, mut y) = (0u32, 0u32);
    let (dpi_x, dpi_y) = if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_ok() {
        (x, y)
    } else {
        (0, 0)
    };
    let r = info.monitorInfo.rcMonitor;
    let w = info.monitorInfo.rcWork;
    let monitors = &mut *(data.0 as *mut Vec<Monitor>);
    monitors.push(Monitor {
        device: u16_string(&info.szDevice),
        primary: info.monitorInfo.dwFlags & 1 != 0,
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
        work_left: w.left,
        work_top: w.top,
        work_right: w.right,
        work_bottom: w.bottom,
        dpi_x,
        dpi_y,
    });
    BOOL(1)
}

// Re-check the actual receiving window after pointer travel: another app may
// have appeared above the intended target during the movement.
fn ensure_point_allowed(x: i32, y: i32) -> Result<(), String> {
    if allow_list_on() {
        ensure_token(&process_image_of(window_at(x, y)))?;
    }
    Ok(())
}

fn paced_wheel(ticks: i32, horizontal: bool, instant: bool) -> Result<(), String> {
    if instant {
        return wheel(ticks, horizontal);
    }
    let count = ticks.unsigned_abs();
    for index in 0..count {
        crate::check_running()?;
        wheel(ticks.signum(), horizontal)?;
        if index + 1 < count {
            let phase = (f64::from(index) + 0.5) / f64::from(count);
            let edge = (phase * 2.0 - 1.0).abs();
            crate::timing::sleep(Duration::from_secs_f64(
                (28.0 + 32.0 * edge * edge) / 1000.0,
            ))?;
        }
    }
    Ok(())
}
