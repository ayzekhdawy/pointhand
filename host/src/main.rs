//! PointHand computer-use host.
//!
//! The TypeScript MCP server spawns this executable and speaks one JSON object
//! per line. This process owns windows, input, the accessibility tree,
//! screenshots, the idle guard, the allow-list, and the Escape stop.

mod desktop;
mod guard;
mod keys;
mod motion;
mod overlay;
mod timing;

use std::io::{self, BufRead, Write};
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use desktop::Session;

static STOPPED: AtomicBool = AtomicBool::new(false);

pub(crate) fn check_running() -> Result<(), String> {
    if STOPPED.load(Ordering::Relaxed) {
        Err("stopped by Escape or emergency stop; restart PointHand to resume.".into())
    } else {
        guard::check_action()
    }
}
pub(crate) fn cancellable_wait(duration: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + duration;
    loop {
        check_running()?;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        overlay::touch();
        thread::sleep(remaining.min(Duration::from_millis(20)));
    }
}

#[derive(Deserialize)]
struct Request {
    id: i64,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    id: i64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn main() {
    if let Some(pid) = argument_pid("--stop-pid") {
        if let Err(error) = signal_stop(pid) {
            eprintln!("{error}");
            exit(1);
        }
        return;
    }
    let parent = parent_pid();
    if let Some(pid) = parent {
        thread::spawn(move || watch_parent(pid));
    }
    let _input_guard = guard::InputGuard::start();
    thread::spawn(watch_stop_event);

    let mut session = match Session::new() {
        Ok(session) => session,
        Err(err) => {
            eprintln!("pointhand host failed to start: {err}");
            exit(1);
        }
    };
    let _overlay = overlay::Overlay::start();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut input = stdin.lock();
    loop {
        let line = match read_request_line(&mut input) {
            Ok(Some(Ok(line))) => line,
            Ok(Some(Err(error))) => {
                write_response(
                    &mut stdout,
                    &Response {
                        id: 0,
                        ok: false,
                        result: None,
                        error: Some(error),
                    },
                );
                continue;
            }
            Ok(None) => break,
            Err(err) => {
                eprintln!("stdin: {err}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                write_response(
                    &mut stdout,
                    &Response {
                        id: 0,
                        ok: false,
                        result: None,
                        error: Some(format!("invalid json: {err}")),
                    },
                );
                continue;
            }
        };
        if STOPPED.load(Ordering::Relaxed)
            && request.method != "close"
            && request.method != "ping"
            && request.method != "overlay_status"
            && request.method != "guard_status"
        {
            write_response(
                &mut stdout,
                &Response {
                    id: request.id,
                    ok: false,
                    result: None,
                    error: Some(
                        "stopped by Escape. No further computer-use actions will run.".into(),
                    ),
                },
            );
            continue;
        }
        let outcome = (|| {
            let _input_lane = guard::InputLock::for_request(&request.method, &request.params)?;
            dispatch(&mut session, &request)
        })();
        guard::end_action();
        let response = match outcome {
            Ok(result) => Response {
                id: request.id,
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(err) => Response {
                id: request.id,
                ok: false,
                result: None,
                error: Some(err),
            },
        };
        if request.method == "close" {
            write_response(&mut stdout, &response);
            break;
        }
        write_response(&mut stdout, &response);
    }
}

const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
// Drain oversized lines without allocating them, so the next request survives.
fn read_request_line(input: &mut impl BufRead) -> io::Result<Option<Result<String, String>>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() && !oversized {
                return Ok(None);
            }
            break;
        }
        let end = available.iter().position(|v| *v == b'\n');
        let count = end.map_or(available.len(), |index| index + 1);
        if !oversized {
            if bytes.len() + count > MAX_REQUEST_BYTES {
                oversized = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&available[..count]);
            }
        }
        input.consume(count);
        if end.is_some() {
            break;
        }
    }
    Ok(Some(if oversized {
        Err("request exceeds 4 MiB".into())
    } else {
        String::from_utf8(bytes).map_err(|_| "request is not valid UTF-8".into())
    }))
}

fn dispatch(session: &mut Session, request: &Request) -> Result<Value, String> {
    let params = if request.params.is_null() {
        &Value::Null
    } else {
        &request.params
    };
    match request.method.as_str() {
        "ping" => Ok(json!({
            "name": "pointhand-computer-use",
            "version": env!("CARGO_PKG_VERSION"),
            "engine": "rust",
            "overlay": "rust",
        })),
        "overlay_status" => Ok(overlay::status()),
        "guard_status" => Ok(guard::status()),
        "get_system_info" => desktop::system_info(),
        "list_windows" => {
            session.list_windows(params.get("query").and_then(|v| v.as_str()).unwrap_or(""))
        }
        "list_apps" => session.list_apps(
            params.get("query").and_then(|v| v.as_str()).unwrap_or(""),
            params
                .get("include_start_menu")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        ),
        "get_window" => session.get_window(number(params, "window_id")?),
        "activate_window" => session.activate(number(params, "window_id")?),
        "get_window_state" => session.window_state(
            number(params, "window_id")?,
            params
                .get("include_screenshot")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            params
                .get("include_text")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            params.get("scale").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32,
        ),
        "screenshot" => session.screenshot(params),
        "click" => session.click(params),
        "move" => session.move_to(params),
        "drag" => session.drag(params),
        "draw" => session.draw(params),
        "draw_strokes" => session.draw_strokes(params),
        "scroll" => session.scroll(params),
        "type_text" => session.type_text(params),
        "press_key" => session.press_key(params),
        "set_value" => session.set_value(params),
        "perform_secondary_action" => session.secondary(params),
        "paste" => session.paste(params),
        "clipboard_read" => session.clipboard_read(),
        "clipboard_write" => session.clipboard_write(
            params
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or("missing text")?,
        ),
        "launch_app" => session.launch(params.get("app").and_then(|v| v.as_str()).unwrap_or("")),
        "batch" => session.batch(params.get("actions").cloned().unwrap_or(Value::Null)),
        "close" => Ok(json!({"bye": true})),
        other => Err(format!("unknown method {other}")),
    }
}

fn number(params: &Value, key: &str) -> Result<i64, String> {
    params
        .get(key)
        .and_then(|value| value.as_i64())
        .ok_or_else(|| format!("missing {key}"))
}

fn write_response(stdout: &mut impl Write, response: &Response) {
    if let Ok(line) = serde_json::to_string(response) {
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
    }
}

fn parent_pid() -> Option<u32> {
    argument_pid("--parent-pid")
}
fn argument_pid(flag: &str) -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == flag {
            return args.next().and_then(|value| value.parse().ok());
        }
    }
    None
}

fn watch_parent(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, false, pid);
        let Ok(handle) = handle else { return };
        let _ = WaitForSingleObject(handle, u32::MAX);
        let _ = CloseHandle(handle);
    }
    STOPPED.store(true, Ordering::Relaxed);
    thread::sleep(Duration::from_millis(150));
    exit(0);
}

fn stop_event_name(pid: u32) -> Vec<u16> {
    format!("Local\\PointHand.Stop.{pid}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}
fn signal_stop(pid: u32) -> Result<(), String> {
    use windows::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
    let name = stop_event_name(pid);
    unsafe {
        let handle = OpenEventW(
            EVENT_MODIFY_STATE,
            false,
            windows::core::PCWSTR(name.as_ptr()),
        )
        .map_err(|e| e.to_string())?;
        let result = SetEvent(handle).map_err(|e| e.to_string());
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        result
    }
}
fn watch_stop_event() {
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
    let name = stop_event_name(std::process::id());
    unsafe {
        let handle = match CreateEventW(None, true, false, windows::core::PCWSTR(name.as_ptr())) {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("PointHand cancellation event unavailable: {error}");
                return;
            }
        };
        let _ = WaitForSingleObject(handle, INFINITE);
        STOPPED.store(true, Ordering::Relaxed);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    #[test]
    fn oversized_line_is_drained_then_next_request_is_read() {
        let mut data = vec![b'x'; MAX_REQUEST_BYTES + 1];
        data.extend_from_slice(b"\n{\"id\":2,\"method\":\"ping\"}\n");
        let mut input = std::io::Cursor::new(data);
        assert!(read_request_line(&mut input).unwrap().unwrap().is_err());
        let request: Request =
            serde_json::from_str(&read_request_line(&mut input).unwrap().unwrap().unwrap())
                .unwrap();
        assert_eq!(request.id, 2);
    }
    #[test]
    fn invalid_utf8_does_not_break_the_next_line() {
        let mut input = std::io::Cursor::new(vec![255, b'\n', b'{', b'}', b'\n']);
        assert!(read_request_line(&mut input).unwrap().unwrap().is_err());
        assert_eq!(
            read_request_line(&mut input).unwrap().unwrap().unwrap(),
            "{}\n"
        );
    }
    #[test]
    fn errors_are_valid_json_even_with_quotes_and_newlines() {
        let mut out = Vec::new();
        write_response(
            &mut out,
            &Response {
                id: 0,
                ok: false,
                result: None,
                error: Some("bad \"key\"\nline".into()),
            },
        );
        assert!(serde_json::from_slice::<Value>(&out).is_ok());
    }
}
