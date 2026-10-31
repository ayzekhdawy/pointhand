//! Search the accessibility tree and wait for windows or elements.
//!
//! `find_elements` walks further than `get_window_state` (3000 elements instead
//! of 400) but returns only the matches. Both replace the cached snapshot, so a
//! returned element_index works with click, set_value and the other tools.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use uiautomation::{UIElement, UITreeWalker};
use windows::Win32::Foundation::HWND;

use super::windows_ops::window_param;
use super::{describe, format_node, top_level_windows, CachedNode, Session};

const FIND_MAX_NODES: usize = 3000;
const FIND_MAX_DEPTH: usize = 32;
const FIND_MAX_CHILDREN: usize = 256;
const FIND_DEFAULT_LIMIT: usize = 50;
const FIND_MAX_LIMIT: usize = 200;
const WAIT_DEFAULT_MS: u64 = 5000;
const WAIT_MAX_MS: u64 = 30_000;
const WAIT_POLL_MS: u64 = 250;

/// What an element must match. Every given field must match.
struct Filter {
    /// Case-insensitive substring of the accessible name.
    name: Option<String>,
    /// Control type such as Button or check box; case, spaces and underscores ignored.
    role: Option<String>,
    /// Exact AutomationId.
    automation_id: Option<String>,
}

impl Filter {
    fn from_params(params: &Value) -> Result<Self, String> {
        let text = |key: &str| {
            params
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let filter = Self {
            name: text("name").map(|name| name.to_lowercase()),
            role: text("role").map(|role| squash(&role)),
            automation_id: text("automation_id"),
        };
        if filter.name.is_none() && filter.role.is_none() && filter.automation_id.is_none() {
            return Err("give at least one of name, role, or automation_id".into());
        }
        Ok(filter)
    }

    fn matches(&self, role: &str, name: &str, automation_id: &str) -> bool {
        if let Some(wanted) = &self.role {
            if squash(role) != *wanted {
                return false;
            }
        }
        if let Some(wanted) = &self.name {
            if !name.to_lowercase().contains(wanted.as_str()) {
                return false;
            }
        }
        if let Some(wanted) = &self.automation_id {
            if automation_id != wanted {
                return false;
            }
        }
        true
    }
}

/// Lowercase letters and digits only, so "Check Box", "check_box" and
/// "CheckBox" compare equal.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

struct Scan {
    matches: Vec<Value>,
    scanned: usize,
    complete: bool,
}

impl Session {
    pub fn find_elements(&mut self, params: &Value) -> Result<Value, String> {
        let window_id = window_param(params)?;
        let filter = Filter::from_params(params)?;
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(FIND_DEFAULT_LIMIT)
            .clamp(1, FIND_MAX_LIMIT);
        self.ensure_allowed(window_id)?;
        let waited_ms = self.settle_before_observation(window_id as isize)?;
        let scan = self.scan(window_id as isize, &filter, limit)?;
        Ok(json!({
            "window_id": window_id,
            "matches": scan.matches,
            "scanned": scan.scanned,
            "complete": scan.complete,
            "waited_ms": waited_ms,
        }))
    }

    /// Poll until a window title or an element appears, or with `gone` until it
    /// disappears. Reads only; Escape stops the wait.
    pub fn wait_for(&mut self, params: &Value) -> Result<Value, String> {
        let timeout_ms = params
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(WAIT_DEFAULT_MS)
            .min(WAIT_MAX_MS);
        let gone = params
            .get("gone")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let title = params
            .get("window_title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_lowercase);
        let started = Instant::now();
        let deadline = started + Duration::from_millis(timeout_ms);
        if let Some(title) = title {
            loop {
                let found = windows_titled(&title);
                if found.is_empty() == gone {
                    return Ok(json!({
                        "window_title": title,
                        "gone": gone,
                        "windows": found,
                        "waited_ms": started.elapsed().as_millis() as u64,
                    }));
                }
                pause_until(deadline, timeout_ms, "window")?;
            }
        }
        let window_id = window_param(params)?;
        let filter = Filter::from_params(params)?;
        self.ensure_allowed(window_id)?;
        loop {
            if describe(window_id as isize).is_none() {
                if gone {
                    return Ok(json!({
                        "window_id": window_id,
                        "gone": true,
                        "window_closed": true,
                        "waited_ms": started.elapsed().as_millis() as u64,
                    }));
                }
                return Err(format!("window {window_id} closed while waiting"));
            }
            let scan = self.scan(window_id as isize, &filter, FIND_DEFAULT_LIMIT)?;
            if scan.matches.is_empty() == gone {
                return Ok(json!({
                    "window_id": window_id,
                    "gone": gone,
                    "matches": scan.matches,
                    "complete": scan.complete,
                    "waited_ms": started.elapsed().as_millis() as u64,
                }));
            }
            pause_until(deadline, timeout_ms, "element")?;
        }
    }

    /// Walk the window, cache every element, and describe the matches in full.
    fn scan(&mut self, hwnd: isize, filter: &Filter, limit: usize) -> Result<Scan, String> {
        let handle = uiautomation::types::Handle::from(HWND(hwnd as *mut std::ffi::c_void));
        let root = self
            .automation
            .element_from_handle(handle)
            .map_err(|err| err.to_string())?;
        self.nodes.clear();
        self.cache_hwnd = hwnd;
        let mut scan = Scan {
            matches: Vec::new(),
            scanned: 0,
            complete: true,
        };
        scan_walk(
            &self.walker,
            &root,
            0,
            filter,
            limit,
            &mut self.nodes,
            &mut scan,
        )?;
        scan.scanned = self.nodes.len();
        Ok(scan)
    }
}

fn scan_walk(
    walker: &UITreeWalker,
    element: &UIElement,
    depth: usize,
    filter: &Filter,
    limit: usize,
    nodes: &mut Vec<CachedNode>,
    scan: &mut Scan,
) -> Result<(), String> {
    if nodes.len() >= FIND_MAX_NODES || depth > FIND_MAX_DEPTH {
        scan.complete = false;
        return Ok(());
    }
    if nodes.len().is_multiple_of(100) {
        crate::check_running()?;
    }
    let index = nodes.len();
    let role = element
        .get_control_type()
        .map(|kind| format!("{kind:?}"))
        .unwrap_or_else(|_| "Unknown".into());
    let name = element.get_name().unwrap_or_default();
    let automation_id = element.get_automation_id().unwrap_or_default();
    let hit = scan.matches.len() < limit && filter.matches(&role, &name, &automation_id);
    let (line, focused, selected) = if hit {
        format_node(element, index, 0)
    } else {
        (format!("[{index}] {role}"), false, false)
    };
    if hit {
        let mut row = json!({"element_index": index, "line": line});
        if !automation_id.is_empty() {
            row["automation_id"] = json!(automation_id);
        }
        scan.matches.push(row);
    }
    nodes.push(CachedNode {
        element: element.clone(),
        line,
        focused,
        selected,
    });
    let mut child = walker.get_first_child(element).ok();
    let mut children = 0;
    while let Some(current) = child {
        if nodes.len() >= FIND_MAX_NODES || children >= FIND_MAX_CHILDREN {
            scan.complete = false;
            break;
        }
        scan_walk(walker, &current, depth + 1, filter, limit, nodes, scan)?;
        children += 1;
        child = walker.get_next_sibling(&current).ok();
    }
    Ok(())
}

fn windows_titled(needle: &str) -> Vec<Value> {
    top_level_windows()
        .into_iter()
        .filter_map(describe)
        .filter(|info| info.pid != std::process::id())
        .filter(|info| info.title.to_lowercase().contains(needle))
        .map(|info| info.to_json())
        .collect()
}

fn pause_until(deadline: Instant, timeout_ms: u64, what: &str) -> Result<(), String> {
    let now = Instant::now();
    if now >= deadline {
        return Err(format!(
            "timeout: the {what} did not reach the requested state within {timeout_ms} ms"
        ));
    }
    let slice = deadline
        .saturating_duration_since(now)
        .min(Duration::from_millis(WAIT_POLL_MS));
    crate::cancellable_wait(slice)
}

#[cfg(test)]
mod tests {
    use super::{squash, Filter};
    use serde_json::json;

    #[test]
    fn a_filter_needs_at_least_one_field() {
        assert!(Filter::from_params(&json!({})).is_err());
        assert!(Filter::from_params(&json!({"name": "  "})).is_err());
        assert!(Filter::from_params(&json!({"role": "Button"})).is_ok());
    }

    #[test]
    fn roles_ignore_case_spaces_and_underscores() {
        assert_eq!(squash("Check Box"), "checkbox");
        assert_eq!(squash("check_box"), "checkbox");
        let filter = Filter::from_params(&json!({"role": "check box"})).unwrap();
        assert!(filter.matches("CheckBox", "Bold", ""));
        assert!(!filter.matches("Button", "Bold", ""));
    }

    #[test]
    fn names_match_as_a_case_insensitive_substring() {
        let filter = Filter::from_params(&json!({"name": "KAYDET"})).unwrap();
        assert!(filter.matches("Button", "Farklı kaydet", ""));
        assert!(!filter.matches("Button", "Aç", ""));
    }

    #[test]
    fn every_given_field_must_match() {
        let filter =
            Filter::from_params(&json!({"role": "Button", "automation_id": "SaveBtn"})).unwrap();
        assert!(filter.matches("Button", "Save", "SaveBtn"));
        assert!(!filter.matches("Button", "Save", "saveBtn"));
        assert!(!filter.matches("Edit", "Save", "SaveBtn"));
    }
}
