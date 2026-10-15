//! Predictable, endpoint-exact pointer motion with gentle acceleration.
use std::time::{Duration, Instant};
use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (10.0 + t * (-15.0 + 6.0 * t))
}

pub fn move_pointer(x: i32, y: i32, instant: bool) -> Result<(), String> {
    let mut start = POINT::default();
    unsafe {
        GetCursorPos(&mut start).map_err(|e| e.to_string())?;
    }
    let distance = (f64::from(x) - f64::from(start.x)).hypot(f64::from(y) - f64::from(start.y));
    let duration = if instant {
        0.0
    } else {
        (60.0 + distance.sqrt() * 12.0).min(450.0)
    };
    let steps = (duration / 8.0).ceil().max(1.0) as u32;
    let began = Instant::now();
    for step in 1..=steps {
        crate::check_running()?;
        let deadline =
            began + Duration::from_secs_f64(duration * f64::from(step) / f64::from(steps) / 1000.0);
        crate::timing::sleep(deadline.saturating_duration_since(Instant::now()))?;
        let t = ease(f64::from(step) / f64::from(steps));
        let px = (f64::from(start.x) + (f64::from(x) - f64::from(start.x)) * t).round() as i32;
        let py = (f64::from(start.y) + (f64::from(y) - f64::from(start.y)) * t).round() as i32;
        crate::desktop::move_pointer_pixel(px, py)?;
        crate::overlay::touch();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_clamped_and_progress_never_reverses() {
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(2.0), 1.0);
        let mut previous = 0.0;
        for i in 0..=1000 {
            let next = ease(f64::from(i) / 1000.0);
            assert!(next >= previous && next <= 1.0);
            previous = next;
        }
        assert!((ease(0.5) - 0.5).abs() < 1e-10);
        assert!(ease(0.1) < 0.1);
        assert!(ease(0.9) > 0.9);
    }
}
