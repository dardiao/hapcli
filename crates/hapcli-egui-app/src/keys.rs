//! 将 egui 键盘事件翻译为终端字节序列。

use eframe::egui::{Key, Modifiers};

use crate::settings::{BackspaceSequence, DeleteSequence};

/// 根据 egui 键盘事件生成需要写入 PTY 的字节。
///
/// 返回 `None` 表示该事件不应转发到终端（例如 Cmd 快捷键、纯修饰键）。
pub fn key_event_to_bytes(
    key: Key,
    modifiers: Modifiers,
    pressed: bool,
    backspace_seq: BackspaceSequence,
    delete_seq: DeleteSequence,
) -> Option<Vec<u8>> {
    if !pressed {
        return None;
    }

    // macOS 的 Cmd 组合键属于应用快捷键（复制/粘贴/新建等），不转发给终端。
    if modifiers.mac_cmd {
        return None;
    }

    // Ctrl + 字母/数字 → ASCII 控制字符。
    if modifiers.ctrl {
        if let Some(byte) = ctrl_byte(key) {
            return Some(vec![byte]);
        }
    }

    // 特殊键序列（方向键/功能键）自带 ESC 与修饰符编码。
    if let Some(bytes) = special_key_sequence(key, modifiers, backspace_seq, delete_seq) {
        return Some(bytes);
    }

    None
}

/// 终端标准特殊键序列。
fn special_key_sequence(
    key: Key,
    modifiers: Modifiers,
    backspace_seq: BackspaceSequence,
    delete_seq: DeleteSequence,
) -> Option<Vec<u8>> {
    use Key::*;

    let arrow = |code: &str, modifiers: Modifiers| {
        let modifier_code = match (modifiers.ctrl, modifiers.alt, modifiers.shift) {
            (false, false, false) => "1",
            (false, false, true) => "1;2",
            (false, true, false) => "1;3",
            (false, true, true) => "1;4",
            (true, false, false) => "1;5",
            (true, false, true) => "1;6",
            (true, true, false) => "1;7",
            (true, true, true) => "1;8",
        };
        if modifier_code == "1" {
            format!("\x1b[{code}")
        } else {
            format!("\x1b[{modifier_code}{code}")
        }
    };

    let tilde = |code: &str, modifiers: Modifiers| {
        let modifier_code = match (modifiers.ctrl, modifiers.alt, modifiers.shift) {
            (false, false, false) => String::new(),
            (false, false, true) => ";2".to_string(),
            (false, true, false) => ";3".to_string(),
            (false, true, true) => ";4".to_string(),
            (true, false, false) => ";5".to_string(),
            (true, false, true) => ";6".to_string(),
            (true, true, false) => ";7".to_string(),
            (true, true, true) => ";8".to_string(),
        };
        format!("\x1b[{code}{modifier_code}~")
    };

    match key {
        Enter => Some(b"\r".to_vec()),
        Backspace => Some(backspace_seq.bytes()),
        Tab => Some(b"\t".to_vec()),
        Escape => Some(vec![0x1b]),
        Delete => Some(delete_seq.bytes()),
        Insert => Some(tilde("2", modifiers).into_bytes()),
        Home => Some(b"\x1b[H".to_vec()),
        End => Some(b"\x1b[F".to_vec()),
        PageUp => Some(tilde("5", modifiers).into_bytes()),
        PageDown => Some(tilde("6", modifiers).into_bytes()),
        ArrowUp => Some(arrow("A", modifiers).into_bytes()),
        ArrowDown => Some(arrow("B", modifiers).into_bytes()),
        ArrowRight => Some(arrow("C", modifiers).into_bytes()),
        ArrowLeft => Some(arrow("D", modifiers).into_bytes()),
        F1 => Some(b"\x1bOP".to_vec()),
        F2 => Some(b"\x1bOQ".to_vec()),
        F3 => Some(b"\x1bOR".to_vec()),
        F4 => Some(b"\x1bOS".to_vec()),
        F5 => Some(b"\x1b[15~".to_vec()),
        F6 => Some(b"\x1b[17~".to_vec()),
        F7 => Some(b"\x1b[18~".to_vec()),
        F8 => Some(b"\x1b[19~".to_vec()),
        F9 => Some(b"\x1b[20~".to_vec()),
        F10 => Some(b"\x1b[21~".to_vec()),
        F11 => Some(b"\x1b[23~".to_vec()),
        F12 => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}

/// Ctrl + 键 → 经典 xterm 控制字节。
fn ctrl_byte(key: Key) -> Option<u8> {
    use Key::*;
    match key {
        Space => Some(0x00),
        Num2 => Some(0x00), // NUL
        Num3 => Some(0x1b), // ESC
        Num4 => Some(0x1c),
        Num5 => Some(0x1d),
        Num6 => Some(0x1e),
        Num7 => Some(0x1f),
        Num8 => Some(0x7f), // DEL
        OpenBracket => Some(0x1b),
        Backslash => Some(0x1c),
        CloseBracket => Some(0x1d),
        Questionmark => Some(0x7f),
        _ => {
            // Ctrl + 字母 → 1..=26（xterm 经典映射）。
            let name = key.name();
            (name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic())
                .then(|| name.as_bytes()[0] & 0x1f)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{BackspaceSequence, DeleteSequence};

    fn kb(key: Key, modifiers: Modifiers, pressed: bool) -> Option<Vec<u8>> {
        key_event_to_bytes(
            key,
            modifiers,
            pressed,
            BackspaceSequence::Delete,
            DeleteSequence::Csi3Tilde,
        )
    }

    #[test]
    fn ctrl_letter_maps_to_control_byte() {
        assert_eq!(kb(Key::C, Modifiers::CTRL, true), Some(vec![0x03]));
        assert_eq!(kb(Key::D, Modifiers::CTRL, true), Some(vec![0x04]));
        assert_eq!(kb(Key::Z, Modifiers::CTRL, true), Some(vec![0x1a]));
    }

    #[test]
    fn ctrl_punctuation_maps_to_xterm_controls() {
        assert_eq!(kb(Key::Num3, Modifiers::CTRL, true), Some(vec![0x1b]));
        assert_eq!(kb(Key::Num8, Modifiers::CTRL, true), Some(vec![0x7f]));
        assert_eq!(kb(Key::OpenBracket, Modifiers::CTRL, true), Some(vec![0x1b]));
    }

    #[test]
    fn special_keys_map_to_escape_sequences() {
        assert_eq!(kb(Key::Enter, Modifiers::NONE, true), Some(b"\r".to_vec()));
        assert_eq!(kb(Key::Backspace, Modifiers::NONE, true), Some(vec![0x7f]));
        assert_eq!(kb(Key::Tab, Modifiers::NONE, true), Some(b"\t".to_vec()));
        assert_eq!(kb(Key::Escape, Modifiers::NONE, true), Some(vec![0x1b]));
        assert_eq!(
            kb(Key::ArrowUp, Modifiers::NONE, true),
            Some(b"\x1b[A".to_vec())
        );
    }

    #[test]
    fn arrow_with_modifiers_uses_xterm_modifier_codes() {
        assert_eq!(
            kb(Key::ArrowLeft, Modifiers::SHIFT, true),
            Some(b"\x1b[1;2D".to_vec())
        );
        assert_eq!(
            kb(Key::ArrowUp, Modifiers::CTRL, true),
            Some(b"\x1b[1;5A".to_vec())
        );
        assert_eq!(
            kb(Key::ArrowDown, Modifiers::ALT, true),
            Some(b"\x1b[1;3B".to_vec())
        );
    }

    #[test]
    fn mac_cmd_shortcuts_are_not_forwarded() {
        assert_eq!(kb(Key::C, Modifiers::COMMAND, true), None);
        assert_eq!(kb(Key::V, Modifiers::COMMAND, true), None);
    }

    #[test]
    fn key_release_is_ignored() {
        assert_eq!(kb(Key::Enter, Modifiers::NONE, false), None);
    }
}
