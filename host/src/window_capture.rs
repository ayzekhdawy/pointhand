//! Capture a window's own pixels, including when another window covers it.
//!
//! The visible desktop is the fallback when window capture is unavailable.
//! Desktop and monitor screenshots stay on that fallback.

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Resource, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_SINGLETHREADED};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

pub struct WindowImage {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// True when the pixels come from the window itself rather than the visible desktop.
    pub own_content: bool,
}

pub fn capture_window(hwnd: isize) -> Result<WindowImage, String> {
    if hwnd == 0 {
        return Err("window capture needs a window".into());
    }
    match capture_own_content(hwnd) {
        Ok(image) => Ok(image),
        Err(own) => match capture_visible(hwnd) {
            Ok(image) => Ok(image),
            Err(visible) => Err(format!("{own}; visible fallback: {visible}")),
        },
    }
}

/// Screen origin of a capture. Prefer the rectangle whose size matches the pixels.
pub fn choose_origin(
    window: (i32, i32, i32, i32),
    extended: Option<(i32, i32, i32, i32)>,
    width: i32,
    height: i32,
) -> (i32, i32) {
    let fits = |rect: (i32, i32, i32, i32)| {
        rect.2.saturating_sub(rect.0) == width && rect.3.saturating_sub(rect.1) == height
    };
    if let Some(rect) = extended.filter(|rect| fits(*rect)) {
        return (rect.0, rect.1);
    }
    if fits(window) {
        return (window.0, window.1);
    }
    extended
        .map(|rect| (rect.0, rect.1))
        .unwrap_or((window.0, window.1))
}

/// Crop a window image to a screen rectangle. The result origin is the crop's top-left.
pub fn crop_rgba(
    image: &WindowImage,
    region: (i32, i32, i32, i32),
) -> Result<(Vec<u8>, u32, u32, i32, i32), String> {
    let image_right = image.left.saturating_add(image.width as i32);
    let image_bottom = image.top.saturating_add(image.height as i32);
    let left = region.0.max(image.left);
    let top = region.1.max(image.top);
    let right = region.2.min(image_right);
    let bottom = region.3.min(image_bottom);
    if right <= left || bottom <= top {
        return Err("screenshot region is outside the window capture".into());
    }
    let width = (right - left) as u32;
    let height = (bottom - top) as u32;
    let origin_x = (left - image.left) as u32;
    let origin_y = (top - image.top) as u32;
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let src = (((origin_y + y) * image.width + origin_x) * 4) as usize;
        let dst = (y * width * 4) as usize;
        let len = (width * 4) as usize;
        out[dst..dst + len].copy_from_slice(&image.rgba[src..src + len]);
    }
    Ok((out, width, height, left, top))
}

fn capture_own_content(hwnd: isize) -> Result<WindowImage, String> {
    if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        return Err("window capture is not supported on this system".into());
    }
    ensure_winrt();
    let (device, context) = d3d_device()?;
    let winrt_device = winrt_device(&device)?;
    let item: GraphicsCaptureItem = unsafe {
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(|err| err.to_string())?
            .CreateForWindow(HWND(hwnd as *mut std::ffi::c_void))
            .map_err(|err| err.to_string())?
    };
    let size = item.Size().map_err(|err| err.to_string())?;
    if size.Width <= 0 || size.Height <= 0 {
        return Err("window capture size is empty".into());
    }
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )
    .map_err(|err| err.to_string())?;
    let session = pool
        .CreateCaptureSession(&item)
        .map_err(|err| err.to_string())?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let _ = session.SetIsBorderRequired(false);
    session.StartCapture().map_err(|err| err.to_string())?;
    let guard = SessionGuard {
        session: Some(session),
        pool: Some(pool),
    };
    let started = Instant::now();
    let frame = loop {
        crate::check_running()?;
        if let Some(pool) = guard.pool.as_ref() {
            if let Ok(frame) = pool.TryGetNextFrame() {
                let content = frame.ContentSize().unwrap_or(size);
                if content.Width > 0 && content.Height > 0 {
                    break frame;
                }
                let _ = frame.Close();
            }
        }
        if started.elapsed() > Duration::from_millis(900) {
            return Err("window capture produced no frame".into());
        }
        std::thread::sleep(Duration::from_millis(15));
    };
    let content = frame.ContentSize().unwrap_or(size);
    let surface = frame.Surface().map_err(|err| err.to_string())?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|err| err.to_string())?;
    let texture: ID3D11Texture2D =
        unsafe { access.GetInterface() }.map_err(|err| err.to_string())?;
    let (rgba, width, height) =
        read_texture(&device, &context, &texture, content.Width, content.Height)?;
    let _ = frame.Close();
    drop(guard);
    let handle = HWND(hwnd as *mut std::ffi::c_void);
    let window = window_rect(handle).unwrap_or((0, 0, width as i32, height as i32));
    let (left, top) = choose_origin(window, extended_frame(handle), width as i32, height as i32);
    Ok(WindowImage {
        left,
        top,
        width,
        height,
        rgba,
        own_content: true,
    })
}

fn capture_visible(hwnd: isize) -> Result<WindowImage, String> {
    let handle = HWND(hwnd as *mut std::ffi::c_void);
    // Only the fallback raises the window. A successful window capture leaves the z-order alone.
    crate::desktop::focus(hwnd)?;
    std::thread::sleep(Duration::from_millis(120));
    let (left, top, right, bottom) = window_rect(handle)?;
    let rect = uiautomation::types::Rect::new(left, top, right, bottom);
    let shot = uiautomation::screenshots::Screenshot::capture_rect(rect)
        .map_err(|err| err.to_string())?
        .to_rgba();
    Ok(WindowImage {
        left,
        top,
        width: shot.width(),
        height: shot.height(),
        rgba: shot.pixels().to_vec(),
        own_content: false,
    })
}

fn ensure_winrt() {
    unsafe {
        let _ = RoInitialize(RO_INIT_SINGLETHREADED);
    }
}

fn d3d_device() -> Result<
    (
        ID3D11Device,
        windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    ),
    String,
> {
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut context = None;
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        if created.is_ok() {
            if let (Some(device), Some(context)) = (device, context) {
                return Ok((device, context));
            }
        }
    }
    Err("could not create a Direct3D device for window capture".into())
}

fn winrt_device(device: &ID3D11Device) -> Result<IDirect3DDevice, String> {
    let dxgi: IDXGIDevice = device.cast().map_err(|err| err.to_string())?;
    let inspectable =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(|err| err.to_string())?;
    inspectable.cast().map_err(|err| err.to_string())
}

fn read_texture(
    device: &ID3D11Device,
    context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    content_width: i32,
    content_height: i32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    let width = (content_width as u32).min(desc.Width).max(1);
    let height = (content_height as u32).min(desc.Height).max(1);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    desc.MiscFlags = 0;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.SampleDesc.Count = 1;
    desc.SampleDesc.Quality = 0;
    let mut staging = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }
        .map_err(|err| err.to_string())?;
    let staging = staging.ok_or("could not create a staging texture")?;
    let source: ID3D11Resource = texture.cast().map_err(|err| err.to_string())?;
    let destination: ID3D11Resource = staging.cast().map_err(|err| err.to_string())?;
    unsafe { context.CopyResource(&destination, &source) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(&destination, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
        .map_err(|err| err.to_string())?;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let pitch = mapped.RowPitch as usize;
    let src = mapped.pData as *const u8;
    if src.is_null() {
        unsafe { context.Unmap(&destination, 0) };
        return Err("window capture mapping was empty".into());
    }
    for y in 0..height as usize {
        let row = unsafe { std::slice::from_raw_parts(src.add(y * pitch), width as usize * 4) };
        for x in 0..width as usize {
            let from = x * 4;
            let to = (y * width as usize + x) * 4;
            rgba[to] = row[from + 2];
            rgba[to + 1] = row[from + 1];
            rgba[to + 2] = row[from];
            rgba[to + 3] = row[from + 3];
        }
    }
    unsafe { context.Unmap(&destination, 0) };
    Ok((rgba, width, height))
}

fn window_rect(hwnd: HWND) -> Result<(i32, i32, i32, i32), String> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|err| err.to_string())?;
    Ok((rect.left, rect.top, rect.right, rect.bottom))
}

fn extended_frame(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    let mut rect = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
    }
    Some((rect.left, rect.top, rect.right, rect.bottom))
}

struct SessionGuard {
    session: Option<GraphicsCaptureSession>,
    pool: Option<Direct3D11CaptureFramePool>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            let _ = session.Close();
        }
        if let Some(pool) = self.pool.take() {
            let _ = pool.Close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{choose_origin, crop_rgba, WindowImage};

    #[test]
    fn origin_follows_the_rectangle_that_matches_the_pixels() {
        let window = (10, 20, 210, 120);
        let extended = Some((18, 28, 198, 112));
        assert_eq!(choose_origin(window, extended, 180, 84), (18, 28));
        assert_eq!(choose_origin(window, extended, 200, 100), (10, 20));
        assert_eq!(choose_origin(window, None, 10, 10), (10, 20));
    }

    #[test]
    fn crop_keeps_only_the_covered_screen_rectangle() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        rgba[0] = 9;
        let image = WindowImage {
            left: 10,
            top: 20,
            width: 4,
            height: 4,
            rgba,
            own_content: true,
        };
        let (pixels, width, height, left, top) = crop_rgba(&image, (10, 20, 12, 22)).unwrap();
        assert_eq!((width, height, left, top), (2, 2, 10, 20));
        assert_eq!(pixels[0], 9);
        assert!(crop_rgba(&image, (100, 100, 110, 110)).is_err());
    }

    /// Two overlapping probe windows. The back window stays covered, and its capture is still its own color.
    #[test]
    fn occluded_window_keeps_its_own_color() {
        use std::time::{Duration, Instant};

        use windows::core::PCWSTR;
        use windows::Graphics::Capture::GraphicsCaptureSession;
        use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
        use windows::Win32::Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetDC, GetStockObject,
            ReleaseDC, BLACK_BRUSH, HBRUSH, PAINTSTRUCT,
        };
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
            GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, PeekMessageW, RegisterClassExW,
            SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
            WindowFromPoint, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOP, MSG, PM_REMOVE,
            SWP_NOMOVE, SWP_NOSIZE, SW_SHOW, WINDOW_EX_STYLE, WM_PAINT, WNDCLASSEXW,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };

        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return;
        }

        unsafe extern "system" fn probe_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if msg == WM_PAINT {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let color = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as u32;
                let brush = CreateSolidBrush(COLORREF(color));
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                FillRect(dc, &client, brush);
                let _ = DeleteObject(brush.into());
                let _ = EndPaint(hwnd, &paint);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        struct Pair(HWND, HWND);
        impl Drop for Pair {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                    let _ = DestroyWindow(self.1);
                }
            }
        }

        let class_name: Vec<u16> = "PointHandProbeCapture\0".encode_utf16().collect();
        let back_title: Vec<u16> = "PointHandProbe back\0".encode_utf16().collect();
        let front_title: Vec<u16> = "PointHandProbe front\0".encode_utf16().collect();
        let instance = match unsafe { GetModuleHandleW(None) } {
            Ok(module) => module,
            Err(_) => return,
        };
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(probe_proc),
            hInstance: instance.into(),
            hbrBackground: HBRUSH(unsafe { GetStockObject(BLACK_BRUSH) }.0),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&class) } == 0
            && std::io::Error::last_os_error().raw_os_error() != Some(1410)
        {
            return;
        }
        let make = |title: &Vec<u16>, x: i32, y: i32, color: u32| unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                x,
                y,
                220,
                160,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap_or_default();
            if !hwnd.0.is_null() {
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, color as isize);
                let _ = ShowWindow(hwnd, SW_SHOW);
                let dc = GetDC(Some(hwnd));
                if !dc.is_invalid() {
                    let brush = CreateSolidBrush(COLORREF(color));
                    let mut client = RECT::default();
                    let _ = GetClientRect(hwnd, &mut client);
                    FillRect(dc, &client, brush);
                    let _ = DeleteObject(brush.into());
                    ReleaseDC(Some(hwnd), dc);
                }
            }
            hwnd
        };
        let back = make(&back_title, 40, 40, 0x00DC_0000);
        let front = make(&front_title, 70, 60, 0x0000_00DC);
        if back.0.is_null() || front.0.is_null() {
            return;
        }
        let _windows = Pair(back, front);
        unsafe {
            let _ = SetWindowPos(front, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = SetForegroundWindow(front);
        }
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(400) {
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        let image = super::capture_window(back.0 as isize).expect("covered window capture");
        assert!(
            image.own_content,
            "covered window fell back to the visible desktop"
        );
        let x = image.width / 2;
        let y = image.height / 2;
        let index = ((y * image.width + x) * 4) as usize;
        let rgba = &image.rgba[index..index + 4];
        assert!(
            rgba[2] > 140 && rgba[0] < 80,
            "covered window center was {:?}, expected its own blue",
            rgba
        );
        let mut rect = RECT::default();
        unsafe { GetWindowRect(back, &mut rect) }.unwrap();
        let hit = unsafe {
            WindowFromPoint(POINT {
                x: (rect.left + rect.right) / 2,
                y: (rect.top + rect.bottom) / 2,
            })
        };
        assert_ne!(hit, back, "screenshot pulled the covered window in front");
        assert_ne!(unsafe { GetForegroundWindow() }, back);
    }
}
