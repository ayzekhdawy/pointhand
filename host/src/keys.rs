//! Key chords. Names follow the usual computer-use aliases (`Return`, `Control_L`, `KP_0`).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: Vec<u16>,
    pub key: u16,
}

const MODIFIERS: &[(&str, u16)] = &[
    ("ctrl", 0x11),
    ("control", 0x11),
    ("control_l", 0x11),
    ("control_r", 0x11),
    ("ctrl_l", 0x11),
    ("ctrl_r", 0x11),
    ("shift", 0x10),
    ("shift_l", 0x10),
    ("shift_r", 0x10),
    ("alt", 0x12),
    ("alt_l", 0x12),
    ("alt_r", 0x12),
    ("win", 0x5B),
    ("meta", 0x5B),
    ("cmd", 0x5B),
    ("super", 0x5B),
    ("super_l", 0x5B),
    ("super_r", 0x5C),
];

pub fn parse_combo(text: &str) -> Result<Combo, String> {
    let trimmed = text.trim();
    if trimmed.ends_with('+') && trimmed != "+" {
        return Err(format!("dangling '+' in key combo: {text:?}"));
    }
    let parts: Vec<String> = trimmed
        .split('+')
        .map(|part| part.trim().to_ascii_lowercase())
        .collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err("empty key combo".into());
    }
    let (last, mods) = parts.split_last().unwrap();
    let mut modifiers = Vec::new();
    for name in mods {
        let code = modifier(name).ok_or_else(|| format!("unknown modifier: {name:?}"))?;
        if !modifiers.contains(&code) {
            modifiers.push(code);
        }
    }
    if mods.is_empty() {
        if let Some(code) = modifier(last) {
            return Ok(Combo {
                modifiers,
                key: code,
            });
        }
    }
    let key = named_key(last).ok_or_else(|| format!("unknown key: {last:?}"))?;
    Ok(Combo { modifiers, key })
}

fn modifier(name: &str) -> Option<u16> {
    MODIFIERS
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, code)| *code)
}

fn named_key(name: &str) -> Option<u16> {
    let named = match name {
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "space" => 0x20,
        "insert" => 0x2D,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "page_up" => 0x21,
        "pagedown" | "page_down" => 0x22,
        "capslock" => 0x14,
        "printscreen" => 0x2C,
        "period" | "greater" => 0xBE,
        "comma" | "less" => 0xBC,
        "slash" | "question" => 0xBF,
        "semicolon" | "colon" => 0xBA,
        "apostrophe" | "quotedbl" => 0xDE,
        "bracketleft" | "braceleft" => 0xDB,
        "bracketright" | "braceright" => 0xDD,
        "backslash" | "bar" => 0xDC,
        "grave" | "asciitilde" => 0xC0,
        "kp_add" | "numpad_add" => 0x6B,
        "kp_subtract" | "numpad_subtract" => 0x6D,
        "kp_multiply" | "numpad_multiply" => 0x6A,
        "kp_divide" | "numpad_divide" => 0x6F,
        "kp_decimal" | "numpad_decimal" => 0x6E,
        "minus" => 0xBD,
        "equal" | "plus" => 0xBB,
        _ => 0,
    };
    if named != 0 {
        return Some(named);
    }
    if let Some(rest) = name.strip_prefix('f') {
        if let Ok(index) = rest.parse::<u16>() {
            if (1..=12).contains(&index) {
                return Some(0x6F + index);
            }
        }
    }
    for prefix in ["kp_", "numpad_"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if let Ok(index) = rest.parse::<u16>() {
                if index < 10 {
                    return Some(0x60 + index);
                }
            }
        }
    }
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphanumeric() {
            return Some(ch.to_ascii_uppercase() as u16);
        }
    }
    None
}

/// Brace syntax kept for the alias table. Input itself is sent as virtual keys.
#[cfg_attr(not(test), allow(dead_code))]
pub fn brace_chord(combo: &Combo) -> String {
    let mut out = String::new();
    for code in &combo.modifiers {
        out.push_str(brace_name(*code));
    }
    out.push_str(brace_name(combo.key));
    out
}

#[cfg_attr(not(test), allow(dead_code))]
fn brace_name(code: u16) -> &'static str {
    match code {
        0xBE => ".",
        0xBC => ",",
        0xBD => "-",
        0xBB => "=",
        0x11 => "{Ctrl}",
        0x10 => "{Shift}",
        0x12 => "{Alt}",
        0x5B => "{Win}",
        0x5C => "{RWin}",
        0x14 => "{CapsLock}",
        0x2C => "{PrintScreen}",
        0xBF => "/",
        0xBA => ";",
        0xDE => "'",
        0xDB => "{",
        0xDD => "}",
        0xDC => "\\",
        0xC0 => "`",
        0x6A => "{NumpadMult}",
        0x6B => "{NumpadAdd}",
        0x6D => "{NumpadSub}",
        0x6E => "{NumpadDot}",
        0x6F => "{NumpadDiv}",
        0x0D => "{Enter}",
        0x09 => "{Tab}",
        0x1B => "{Escape}",
        0x08 => "{Backspace}",
        0x2E => "{Delete}",
        0x20 => " ",
        0x2D => "{Insert}",
        0x26 => "{Up}",
        0x28 => "{Down}",
        0x25 => "{Left}",
        0x27 => "{Right}",
        0x24 => "{Home}",
        0x23 => "{End}",
        0x21 => "{PageUp}",
        0x22 => "{PageDown}",
        0x70..=0x7B => match code - 0x6F {
            1 => "{F1}",
            2 => "{F2}",
            3 => "{F3}",
            4 => "{F4}",
            5 => "{F5}",
            6 => "{F6}",
            7 => "{F7}",
            8 => "{F8}",
            9 => "{F9}",
            10 => "{F10}",
            11 => "{F11}",
            _ => "{F12}",
        },
        0x41..=0x5A => match code {
            0x41 => "a",
            0x42 => "b",
            0x43 => "c",
            0x44 => "d",
            0x45 => "e",
            0x46 => "f",
            0x47 => "g",
            0x48 => "h",
            0x49 => "i",
            0x4A => "j",
            0x4B => "k",
            0x4C => "l",
            0x4D => "m",
            0x4E => "n",
            0x4F => "o",
            0x50 => "p",
            0x51 => "q",
            0x52 => "r",
            0x53 => "s",
            0x54 => "t",
            0x55 => "u",
            0x56 => "v",
            0x57 => "w",
            0x58 => "x",
            0x59 => "y",
            _ => "z",
        },
        0x60..=0x69 => match code - 0x60 {
            0 => "{Numpad0}",
            1 => "{Numpad1}",
            2 => "{Numpad2}",
            3 => "{Numpad3}",
            4 => "{Numpad4}",
            5 => "{Numpad5}",
            6 => "{Numpad6}",
            7 => "{Numpad7}",
            8 => "{Numpad8}",
            _ => "{Numpad9}",
        },
        0x30..=0x39 => match code {
            0x30 => "0",
            0x31 => "1",
            0x32 => "2",
            0x33 => "3",
            0x34 => "4",
            0x35 => "5",
            0x36 => "6",
            0x37 => "7",
            0x38 => "8",
            _ => "9",
        },
        _ => "{Unknown}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sky_style_chord() {
        let combo = parse_combo("Control_L+a").unwrap();
        assert_eq!(combo.modifiers, vec![0x11]);
        assert_eq!(combo.key, b'A' as u16);
        assert_eq!(brace_chord(&combo), "{Ctrl}a");
    }

    #[test]
    fn rejects_empty_components_and_deduplicates_modifiers() {
        for bad in ["", "+", "+a", "ctrl++a", "ctrl+", "hyper+a"] {
            assert!(parse_combo(bad).is_err(), "{bad}");
        }
        assert_eq!(
            parse_combo("Ctrl+Control_L+a").unwrap().modifiers,
            vec![0x11]
        );
    }

    #[test]
    fn aliases_have_compatible_brace_names() {
        for (name, code, brace) in [
            ("Return", 0x0D, "{Enter}"),
            ("Control_R", 0x11, "{Ctrl}"),
            ("Shift_L", 0x10, "{Shift}"),
            ("Alt_L", 0x12, "{Alt}"),
            ("super", 0x5B, "{Win}"),
            ("KP_0", 0x60, "{Numpad0}"),
            ("page_up", 0x21, "{PageUp}"),
            ("period", 0xBE, "."),
            ("slash", 0xBF, "/"),
            ("KP_Add", 0x6B, "{NumpadAdd}"),
            ("capslock", 0x14, "{CapsLock}"),
            ("printscreen", 0x2C, "{PrintScreen}"),
        ] {
            let combo = parse_combo(name).unwrap();
            assert_eq!(combo.key, code);
            assert_eq!(brace_chord(&combo), brace);
        }
    }

    #[test]
    fn parses_return_and_numpad() {
        assert_eq!(parse_combo("Return").unwrap().key, 0x0D);
        assert_eq!(parse_combo("KP_0").unwrap().key, 0x60);
        assert_eq!(parse_combo("numpad_9").unwrap().key, 0x69);
    }

    #[test]
    fn parses_modifier_aliases_and_page_keys() {
        let combo = parse_combo("ctrl_r+super+page_up").unwrap();
        assert_eq!(combo.modifiers, vec![0x11, 0x5B]);
        assert_eq!(combo.key, 0x21);
        assert_eq!(parse_combo("period").unwrap().key, 0xBE);
        assert!(parse_combo("ctrl+").is_err());
    }
}
