//! Short input pacing without the coarse rounding of ordinary Windows Sleep.
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CancelWaitableTimer, CreateWaitableTimerExW, SetWaitableTimer, WaitForSingleObject,
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS,
};
struct Timer(HANDLE);
impl Timer {
    fn new() -> Result<Self, String> {
        unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
            .or_else(|_| CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0))
            .map(Self)
            .map_err(|error| error.to_string())
        }
    }
    fn wait(&self, duration: Duration) -> Result<(), String> {
        if duration.is_zero() {
            return Ok(());
        }
        let due = -i64::try_from((duration.as_nanos() / 100).max(1))
            .map_err(|_| "timer duration too large")?;
        unsafe {
            SetWaitableTimer(self.0, &due, 0, None, None, false)
                .map_err(|error| error.to_string())?;
            if WaitForSingleObject(self.0, 250) != WAIT_OBJECT_0 {
                return Err("input pacing timer failed".into());
            }
        }
        Ok(())
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        unsafe {
            let _ = CancelWaitableTimer(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}
thread_local! {static TIMER:Result<Timer,String>=Timer::new();}
pub fn sleep(duration: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + duration;
    loop {
        crate::check_running()?;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        let chunk = remaining.min(Duration::from_millis(20));
        TIMER.with(|timer| match timer {
            Ok(timer) => timer.wait(chunk),
            Err(_) => {
                std::thread::sleep(chunk);
                Ok(())
            }
        })?;
    }
}
