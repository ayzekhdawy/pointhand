//! Native glass frame and cursor. HWND and GDI resources belong to one thread.
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, LazyLock, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
const EDGE: i32 = 24;
const CURSOR: i32 = 128;
const PAD: i32 = 48;
const PERIOD: Duration = Duration::from_nanos(16_666_667);
#[derive(Default)]
struct Activity {
    last: Option<Instant>,
    click: u64,
    position: Option<(i32, i32)>,
}
static ACTIVITY: LazyLock<Mutex<Activity>> = LazyLock::new(|| Mutex::new(Activity::default()));
static READY: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static RENDER_US: AtomicU64 = AtomicU64::new(0);
static INTERVAL_US: AtomicU64 = AtomicU64::new(0);
static INTERVAL_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static INTERVAL_COUNT: AtomicU64 = AtomicU64::new(0);
static MAX_INTERVAL_US: AtomicU64 = AtomicU64::new(0);
static ERROR: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
pub fn signal(x: i32, y: i32, clicked: bool) {
    if let Ok(mut state) = ACTIVITY.lock() {
        state.last = Some(Instant::now());
        if x != i32::MIN && y != i32::MIN {
            state.position = Some((x, y));
        }
        if clicked {
            state.click = state.click.wrapping_add(1);
        }
    }
}
pub fn touch() {
    signal(i32::MIN, i32::MIN, false);
}
pub fn status() -> Value {
    let total = INTERVAL_TOTAL_US.load(Ordering::Relaxed);
    let average_fps = if total > 0 {
        Some(INTERVAL_COUNT.load(Ordering::Relaxed) as f64 * 1_000_000.0 / total as f64)
    } else {
        None
    };
    json!({"renderer":"rust","ready":READY.load(Ordering::Relaxed),
    "enabled":std::env::var("POINTHAND_OVERLAY").as_deref()!=Ok("0"),"target_fps":60,
    "cursor_frames":FRAMES.load(Ordering::Relaxed),"last_render_us":RENDER_US.load(Ordering::Relaxed),
        "last_frame_interval_us":INTERVAL_US.load(Ordering::Relaxed),
        "average_active_fps":average_fps,"max_frame_interval_us":MAX_INTERVAL_US.load(Ordering::Relaxed),
        "error":ERROR.lock().ok().and_then(|v|v.clone())})
}
pub struct Overlay {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Overlay {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = if std::env::var("POINTHAND_OVERLAY").as_deref() == Ok("0") {
            None
        } else {
            let stopped = stop.clone();
            Some(thread::spawn(move || {
                if let Err(error) = run(&stopped) {
                    eprintln!("PointHand native overlay unavailable: {error}");
                    if let Ok(mut slot) = ERROR.lock() {
                        *slot = Some(error);
                    }
                }
                READY.store(false, Ordering::Relaxed);
            }))
        };
        Self { stop, thread }
    }
}
impl Drop for Overlay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
struct Surface {
    hwnd: HWND,
    screen: HDC,
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}
impl Surface {
    fn new(instance: HINSTANCE, width: i32, height: i32) -> Result<Self, String> {
        if width <= 0 || height <= 0 {
            return Err("invalid overlay dimensions".into());
        }
        let count = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .filter(|v| *v <= 64 * 1024 * 1024)
            .ok_or("overlay surface is too large")?;
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST,
                w!("PointHand.NativeOverlay"),
                w!("PointHand overlay"),
                WS_POPUP | WS_DISABLED,
                0,
                0,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|e| e.to_string())?;
            let mut surface = Self {
                hwnd,
                screen: GetDC(None),
                dc: HDC::default(),
                bitmap: HBITMAP::default(),
                previous: HGDIOBJ::default(),
                bits: std::ptr::null_mut(),
                width,
                height,
                pixels: vec![0; count],
            };
            if surface.screen.0.is_null() {
                return Err("overlay GetDC failed".into());
            }
            surface.dc = CreateCompatibleDC(Some(surface.screen));
            if surface.dc.0.is_null() {
                return Err("overlay CreateCompatibleDC failed".into());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            surface.bitmap = CreateDIBSection(
                Some(surface.screen),
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )
            .map_err(|e| e.to_string())?;
            surface.bits = bits.cast();
            if surface.bits.is_null() {
                return Err("overlay DIB has no storage".into());
            }
            surface.previous = SelectObject(surface.dc, HGDIOBJ(surface.bitmap.0));
            if surface.previous.0.is_null() || surface.previous.0 as isize == -1 {
                return Err("overlay SelectObject failed".into());
            }
            if std::env::var("POINTHAND_OVERLAY_CAPTURE").as_deref() != Ok("1") {
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
                    .map_err(|e| e.to_string())?;
            }
            Ok(surface)
        }
    }
    fn present(&self, x: i32, y: i32) -> Result<(), String> {
        unsafe {
            std::ptr::copy_nonoverlapping(self.pixels.as_ptr(), self.bits, self.pixels.len());
            UpdateLayeredWindow(
                self.hwnd,
                Some(self.screen),
                Some(&POINT { x, y }),
                Some(&SIZE {
                    cx: self.width,
                    cy: self.height,
                }),
                Some(self.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                }),
                ULW_ALPHA,
            )
            .map_err(|e| e.to_string())
        }
    }
    fn show(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.dc.0.is_null() {
                if !self.previous.0.is_null() && self.previous.0 as isize != -1 {
                    SelectObject(self.dc, self.previous);
                }
                if !self.bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
                }
                let _ = DeleteDC(self.dc);
            }
            if !self.screen.0.is_null() {
                ReleaseDC(None, self.screen);
            }
            if !self.hwnd.0.is_null() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}
// Premultiplied BGRA source-over. No allocation inside the cursor loop.
fn blend(pixels: &mut [u8], index: usize, rgb: [u8; 3], alpha: u8) {
    let a = alpha as u32;
    for (i, s) in [rgb[2], rgb[1], rgb[0]].into_iter().enumerate() {
        pixels[index + i] =
            ((s as u32 * a + pixels[index + i] as u32 * (255 - a) + 127) / 255) as u8;
    }
    pixels[index + 3] = (a + (pixels[index + 3] as u32 * (255 - a) + 127) / 255) as u8;
}
fn gradient(position: f32, phase: f32) -> [u8; 3] {
    let t = 1. - ((position + phase).rem_euclid(1.) * 2. - 1.).abs();
    let a = [59., 47., 217.];
    let b = [34., 211., 238.];
    std::array::from_fn(|i| (a[i] + (b[i] - a[i]) * t).round() as u8)
}
fn rounded_distance(x: f32, y: f32, width: i32, height: i32) -> f32 {
    let radius = 22.0;
    let half_x = (width - 1) as f32 / 2.0;
    let half_y = (height - 1) as f32 / 2.0;
    let qx = (x - half_x).abs() - (half_x - 5.0 - radius);
    let qy = (y - half_y).abs() - (half_y - 5.0 - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}
fn paint_edge(surface: &mut Surface, side: usize, phase: f32, width: i32, height: i32) {
    surface.pixels.fill(0);
    for y in 0..surface.height {
        for x in 0..surface.width {
            let (gx, gy) = match side {
                0 => (x, y),
                1 => (x, height - EDGE + y),
                2 => (x, y + EDGE),
                _ => (width - EDGE + x, y + EDGE),
            };
            let perimeter = 2.0 * (width + height) as f32;
            let along = match side {
                0 => gx as f32,
                1 => (2 * width + height - gx) as f32,
                2 => (2 * width + 2 * height - gy) as f32,
                _ => (width + gy) as f32,
            } / perimeter;
            let d = rounded_distance(gx as f32, gy as f32, width, height).abs();
            let alpha = if d <= 1.2 {
                240
            } else {
                (110. * (-d * d / 60.).exp()) as u8
            };
            blend(
                &mut surface.pixels,
                ((y * surface.width + x) * 4) as usize,
                gradient(along, phase),
                alpha,
            );
        }
    }
}
fn inside_polygon(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut previous = points[points.len() - 1];
    for &point in points {
        if (point.1 > y) != (previous.1 > y)
            && x < (previous.0 - point.0) * (y - point.1) / (previous.1 - point.1) + point.0
        {
            inside = !inside;
        }
        previous = point;
    }
    inside
}
fn cursor_sprite() -> Vec<u8> {
    let shape = [
        (0., 0.),
        (0., 25.),
        (6., 19.),
        (12., 30.),
        (16., 28.),
        (11., 18.),
        (20., 18.),
    ];
    let mut pixels = vec![0; (CURSOR * CURSOR * 4) as usize];
    for y in 0..CURSOR {
        for x in 0..CURSOR {
            let fx = x as f32 - PAD as f32;
            let fy = y as f32 - PAD as f32;
            let index = ((y * CURSOR + x) * 4) as usize;
            blend(
                &mut pixels,
                index,
                [34, 211, 238],
                (100. * (-(fx * fx + fy * fy) / 420.).exp()) as u8,
            );
            let mut fill = 0;
            let mut border = 0;
            for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let px = fx + dx;
                let py = fy + dy;
                if inside_polygon(px, py, &shape) {
                    fill += 1;
                } else if [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)]
                    .iter()
                    .any(|(ox, oy)| inside_polygon(px + ox, py + oy, &shape))
                {
                    border += 1;
                }
            }
            if border > 0 {
                blend(
                    &mut pixels,
                    index,
                    [255, 255, 255],
                    (border * 255 / 4) as u8,
                );
            }
            if fill > 0 {
                blend(&mut pixels, index, [59, 47, 217], (fill * 255 / 4) as u8);
            }
        }
    }
    pixels
}
fn dot(surface: &mut Surface, cx: f32, cy: f32, radius: f32, alpha: u8, ring: bool) {
    let extent = radius.ceil() as i32 + 3;
    for y in (cy as i32 - extent).max(0)..=(cy as i32 + extent).min(surface.height - 1) {
        for x in (cx as i32 - extent).max(0)..=(cx as i32 + extent).min(surface.width - 1) {
            let distance = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let coverage = if ring {
                (1.5 - (distance - radius).abs()).clamp(0., 1.)
            } else {
                (radius + 1. - distance).clamp(0., 1.)
            };
            blend(
                &mut surface.pixels,
                ((y * surface.width + x) * 4) as usize,
                [34, 211, 238],
                (alpha as f32 * coverage) as u8,
            );
        }
    }
}
fn label(surface: &mut Surface) -> Result<(), String> {
    unsafe {
        std::ptr::write_bytes(surface.bits, 0, surface.pixels.len());
        let font = CreateFontW(
            18,
            0,
            0,
            0,
            600,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            0,
            w!("Segoe UI"),
        );
        if font.0.is_null() {
            return Err("overlay font creation failed".into());
        }
        let old = SelectObject(surface.dc, HGDIOBJ(font.0));
        SetBkMode(surface.dc, TRANSPARENT);
        SetTextColor(surface.dc, COLORREF(0xFFFFFF));
        let mut text = "PointHand is controlling this PC  -  touch the mouse to pause"
            .encode_utf16()
            .collect::<Vec<_>>();
        let mut rect = RECT {
            left: 36,
            top: 0,
            right: surface.width - 12,
            bottom: surface.height,
        };
        DrawTextW(
            surface.dc,
            &mut text,
            &mut rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
        );
        let _ = GdiFlush();
        let mask = std::slice::from_raw_parts(surface.bits, surface.pixels.len()).to_vec();
        SelectObject(surface.dc, old);
        let _ = DeleteObject(HGDIOBJ(font.0));
        for y in 0..surface.height {
            for x in 0..surface.width {
                let index = ((y * surface.width + x) * 4) as usize;
                let cx = (x as f32).clamp(22., surface.width as f32 - 22.);
                let dist = ((x as f32 - cx).powi(2) + (y as f32 - 22.).powi(2)).sqrt();
                if dist < 22. {
                    blend(&mut surface.pixels, index, [26, 24, 58], 180);
                }
                if (dist - 21.).abs() < 0.7 {
                    blend(&mut surface.pixels, index, [160, 170, 255], 150);
                }
                blend(
                    &mut surface.pixels,
                    index,
                    [240, 238, 255],
                    mask[index].max(mask[index + 1]).max(mask[index + 2]),
                );
            }
        }
        dot(surface, 22., 22., 6., 255, false);
    }
    Ok(())
}
fn bounds() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}
struct Frame {
    edges: Vec<Surface>,
    label: Surface,
    bounds: (i32, i32, i32, i32),
}
impl Frame {
    fn new(instance: HINSTANCE, bounds: (i32, i32, i32, i32)) -> Result<Self, String> {
        let (_, _, w, h) = bounds;
        if w < EDGE * 2 || h < EDGE * 2 {
            return Err("desktop is too small for overlay".into());
        }
        let mut label_surface = Surface::new(instance, 560, 44)?;
        label(&mut label_surface)?;
        Ok(Self {
            edges: vec![
                Surface::new(instance, w, EDGE)?,
                Surface::new(instance, w, EDGE)?,
                Surface::new(instance, EDGE, h - EDGE * 2)?,
                Surface::new(instance, EDGE, h - EDGE * 2)?,
            ],
            label: label_surface,
            bounds,
        })
    }
    fn paint(&mut self, phase: f32) -> Result<(), String> {
        let (x, y, w, h) = self.bounds;
        let origins = [
            (x, y),
            (x, y + h - EDGE),
            (x, y + EDGE),
            (x + w - EDGE, y + EDGE),
        ];
        for (side, edge) in self.edges.iter_mut().enumerate() {
            paint_edge(edge, side, phase, w, h);
            edge.present(origins[side].0, origins[side].1)?;
        }
        self.label.present(x + (w - self.label.width) / 2, y + 22)
    }
    fn show(&self, visible: bool) {
        for edge in &self.edges {
            edge.show(visible);
        }
        self.label.show(visible);
    }
}
fn run(stop: &AtomicBool) -> Result<(), String> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.to_string())?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("PointHand.NativeOverlay"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(format!(
                "overlay RegisterClass: {}",
                windows::core::Error::from_thread()
            ));
        }
        let result = (|| {
            let mut frame = Frame::new(instance, bounds())?;
            let mut cursor = Surface::new(instance, CURSOR, CURSOR)?;
            let sprite = cursor_sprite();
            let mut visible = false;
            let started = Instant::now();
            let mut deadline = started;
            let mut last_frame = started - Duration::from_secs(1);
            let mut last_cursor = None;
            let mut seen_click = 0;
            let mut ripple: Option<Instant> = None;
            let mut trail = VecDeque::with_capacity(7);
            READY.store(true, Ordering::Relaxed);
            while !stop.load(Ordering::Relaxed) {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return Ok(());
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let now = Instant::now();
                let (active, click, fallback) = ACTIVITY
                    .lock()
                    .map(|s| {
                        (
                            s.last.is_some_and(|t| {
                                now.duration_since(t) < Duration::from_millis(1200)
                            }),
                            s.click,
                            s.position,
                        )
                    })
                    .unwrap_or((false, 0, None));
                let active = active && !crate::STOPPED.load(Ordering::Relaxed);
                if active {
                    let draw_started = Instant::now();
                    if frame.bounds != bounds() {
                        frame = Frame::new(instance, bounds())?;
                        last_frame = now - Duration::from_secs(1);
                        visible = false;
                    }
                    if now.duration_since(last_frame) >= Duration::from_millis(100) {
                        frame.paint(started.elapsed().as_secs_f32() * 0.35)?;
                        last_frame = now;
                    }
                    if click != seen_click {
                        ripple = Some(now);
                        seen_click = click;
                    }
                    let mut point = POINT::default();
                    if GetCursorPos(&mut point).is_err() {
                        if let Some((x, y)) = fallback {
                            point = POINT { x, y };
                        }
                    }
                    cursor.pixels.copy_from_slice(&sprite);
                    if trail.back() != Some(&(point.x, point.y)) {
                        trail.push_back((point.x, point.y));
                        if trail.len() > 7 {
                            trail.pop_front();
                        }
                    }
                    for (i, &(x, y)) in trail.iter().enumerate() {
                        dot(
                            &mut cursor,
                            (x - point.x + PAD) as f32,
                            (y - point.y + PAD) as f32,
                            2. + i as f32 * 0.4,
                            20 + i as u8 * 8,
                            false,
                        );
                    }
                    if let Some(at) = ripple {
                        let p = now.duration_since(at).as_secs_f32() / 0.5;
                        if p < 1. {
                            dot(
                                &mut cursor,
                                PAD as f32,
                                PAD as f32,
                                10. + p * 35.,
                                (220. * (1. - p)) as u8,
                                true,
                            );
                        } else {
                            ripple = None;
                        }
                    }
                    cursor.present(point.x - PAD, point.y - PAD)?;
                    if !visible {
                        frame.show(true);
                        cursor.show(true);
                        visible = true;
                    }
                    if let Some(last) = last_cursor {
                        let interval = now.duration_since(last).as_micros() as u64;
                        INTERVAL_US.store(interval, Ordering::Relaxed);
                        INTERVAL_TOTAL_US.fetch_add(interval, Ordering::Relaxed);
                        INTERVAL_COUNT.fetch_add(1, Ordering::Relaxed);
                        MAX_INTERVAL_US.fetch_max(interval, Ordering::Relaxed);
                    }
                    last_cursor = Some(now);
                    FRAMES.fetch_add(1, Ordering::Relaxed);
                    RENDER_US.store(draw_started.elapsed().as_micros() as u64, Ordering::Relaxed);
                } else {
                    if visible {
                        frame.show(false);
                        cursor.show(false);
                        visible = false;
                    }
                    trail.clear();
                    last_cursor = None;
                }
                deadline += PERIOD;
                if deadline < Instant::now() {
                    deadline = Instant::now();
                }
                crate::timing::frame_sleep(deadline.saturating_duration_since(Instant::now()));
            }
            Ok(())
        })();
        let _ = UnregisterClassW(w!("PointHand.NativeOverlay"), Some(instance));
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounded_frame_has_clear_corners_and_lit_top_edge() {
        assert!(rounded_distance(0.0, 0.0, 1920, 1080) > 12.0);
        assert!(rounded_distance(960.0, 5.0, 1920, 1080).abs() < 0.01);
        assert!(rounded_distance(960.0, 540.0, 1920, 1080) < -100.0);
    }
    #[test]
    fn gradient_is_seamless() {
        assert_eq!(gradient(0., 0.), gradient(1., 0.));
        assert_eq!(gradient(0., 0.), [59, 47, 217]);
        assert_eq!(gradient(0.5, 0.), [34, 211, 238]);
    }
    #[test]
    fn sprite_is_premultiplied_and_clear_outside() {
        let p = cursor_sprite();
        assert_eq!(p.len(), (CURSOR * CURSOR * 4) as usize);
        assert_eq!(&p[p.len() - 4..], &[0, 0, 0, 0]);
        for pixel in p.chunks_exact(4) {
            assert!(pixel[..3].iter().all(|v| *v <= pixel[3]));
        }
        let index = (((PAD + 8) * CURSOR + PAD + 2) * 4) as usize;
        assert_eq!(p[index + 3], 255);
    }
    #[test]
    fn blending_does_not_overflow() {
        let mut p = vec![0; 4];
        blend(&mut p, 0, [255, 255, 255], 128);
        blend(&mut p, 0, [255, 255, 255], 255);
        assert_eq!(p, [255, 255, 255, 255]);
    }
}
