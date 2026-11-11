//! US 配列の物理キー表（スキャンコード・セット 1）と、シナリオのキー指定の解釈。
//! `SendInput` を `KEYEVENTF_SCANCODE` で送るため、仮想キーではなくスキャンコードで持つ。

/// 物理キー 1 つ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    /// スキャンコード
    pub scan: u16,
    /// 拡張キー（0xE0 プレフィックス）か
    pub extended: bool,
}

const fn plain(scan: u16) -> Stroke {
    Stroke {
        scan,
        extended: false,
    }
}

const fn ext(scan: u16) -> Stroke {
    Stroke {
        scan,
        extended: true,
    }
}

/// Shift のスキャンコード（左）
pub const SHIFT: Stroke = plain(0x2A);
/// Ctrl のスキャンコード（左）
pub const CTRL: Stroke = plain(0x1D);
/// Alt のスキャンコード（左）
pub const ALT: Stroke = plain(0x38);

/// 1 文字を送るための物理キーと Shift の要否（US 配列）。表にない文字は `None`
pub fn char_stroke(c: char) -> Option<(Stroke, bool)> {
    const ROW_Q: &str = "qwertyuiop[]";
    const ROW_A: &str = "asdfghjkl;'";
    const ROW_Z: &str = "zxcvbnm,./";
    const DIGITS: &str = "1234567890-=";
    // Shift を押したときの記号（DIGITS と同じ並び）
    const DIGITS_SHIFTED: &str = "!@#$%^&*()_+";
    const ROW_Q_SHIFTED: &str = "QWERTYUIOP{}";
    const ROW_A_SHIFTED: &str = "ASDFGHJKL:\"";
    const ROW_Z_SHIFTED: &str = "ZXCVBNM<>?";

    let rows: [(&str, &str, u16); 4] = [
        (DIGITS, DIGITS_SHIFTED, 0x02),
        (ROW_Q, ROW_Q_SHIFTED, 0x10),
        (ROW_A, ROW_A_SHIFTED, 0x1E),
        (ROW_Z, ROW_Z_SHIFTED, 0x2C),
    ];
    for (plain_row, shifted_row, base) in rows {
        if let Some(i) = plain_row.chars().position(|x| x == c) {
            return Some((plain(base + i as u16), false));
        }
        if let Some(i) = shifted_row.chars().position(|x| x == c) {
            return Some((plain(base + i as u16), true));
        }
    }
    match c {
        ' ' => Some((plain(0x39), false)),
        '\\' => Some((plain(0x2B), false)),
        '|' => Some((plain(0x2B), true)),
        '`' => Some((plain(0x29), false)),
        '~' => Some((plain(0x29), true)),
        _ => None,
    }
}

/// 特殊キー名（大文字小文字を区別しない）→ 物理キー
fn named_stroke(name: &str) -> Option<Stroke> {
    Some(match name.to_ascii_lowercase().as_str() {
        "return" | "enter" => plain(0x1C),
        "space" => plain(0x39),
        "backspace" => plain(0x0E),
        "escape" | "esc" => plain(0x01),
        "tab" => plain(0x0F),
        "left" => ext(0x4B),
        "right" => ext(0x4D),
        "up" => ext(0x48),
        "down" => ext(0x50),
        "delete" => ext(0x53),
        _ => return None,
    })
}

/// `Shift+Left` のようなキー指定。修飾（押す順）と本体
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpec {
    /// 先に押す修飾キー
    pub modifiers: Vec<Stroke>,
    /// 本体のキー
    pub key: Stroke,
}

/// キー指定を解釈する。`+` で連結し、最後が本体、前は修飾（`Shift` / `Ctrl` / `Alt`）。
/// 本体は特殊キー名か、1 文字（US 配列で Shift が要る文字は Shift を補う）
pub fn parse_key(spec: &str) -> Result<KeySpec, String> {
    // 本体が「+」自身の場合（"Shift++" など）を許すため、末尾の + は本体として扱う
    let (mods_part, body) = if let Some(head) = spec.strip_suffix("++") {
        (head.to_owned(), "+")
    } else if spec == "+" {
        (String::new(), "+")
    } else {
        match spec.rsplit_once('+') {
            Some((m, b)) => (m.to_owned(), b),
            None => (String::new(), spec),
        }
    };
    let mut modifiers = Vec::new();
    for m in mods_part.split('+').filter(|m| !m.is_empty()) {
        modifiers.push(match m.to_ascii_lowercase().as_str() {
            "shift" => SHIFT,
            "ctrl" | "control" => CTRL,
            "alt" | "option" => ALT,
            other => return Err(format!("Windows で扱えない修飾キー: {other}")),
        });
    }
    if let Some(stroke) = named_stroke(body) {
        return Ok(KeySpec {
            modifiers,
            key: stroke,
        });
    }
    let mut chars = body.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => {
            let (stroke, shift) = char_stroke(c).ok_or_else(|| format!("未対応のキー: {body}"))?;
            if shift && !modifiers.contains(&SHIFT) {
                modifiers.insert(0, SHIFT);
            }
            Ok(KeySpec {
                modifiers,
                key: stroke,
            })
        }
        _ => Err(format!("未対応のキー名: {body}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits_use_us_scancodes() {
        assert_eq!(char_stroke('a'), Some((plain(0x1E), false)));
        assert_eq!(char_stroke('A'), Some((plain(0x1E), true)));
        assert_eq!(char_stroke('q'), Some((plain(0x10), false)));
        assert_eq!(char_stroke('m'), Some((plain(0x32), false)));
        assert_eq!(char_stroke('1'), Some((plain(0x02), false)));
        assert_eq!(char_stroke('0'), Some((plain(0x0B), false)));
        assert_eq!(char_stroke('!'), Some((plain(0x02), true)));
        assert_eq!(char_stroke('-'), Some((plain(0x0C), false)));
        assert_eq!(char_stroke(','), Some((plain(0x33), false)));
        assert_eq!(char_stroke('?'), Some((plain(0x35), true)));
        assert_eq!(char_stroke(' '), Some((plain(0x39), false)));
        assert_eq!(char_stroke('あ'), None);
    }

    #[test]
    fn every_ascii_printable_char_has_a_stroke() {
        for c in (0x20u8..0x7F).map(char::from) {
            assert!(char_stroke(c).is_some(), "{c:?}");
        }
    }

    #[test]
    fn parses_special_keys_with_modifiers() {
        let k = parse_key("Return").unwrap();
        assert_eq!(k.key, plain(0x1C));
        assert!(k.modifiers.is_empty());
        let k = parse_key("Shift+Left").unwrap();
        assert_eq!(k.modifiers, vec![SHIFT]);
        assert_eq!(k.key, ext(0x4B));
        let k = parse_key("Ctrl+Shift+Right").unwrap();
        assert_eq!(k.modifiers, vec![CTRL, SHIFT]);
        assert_eq!(parse_key("Ctrl+a").unwrap().key, plain(0x1E));
        assert!(parse_key("Cmd+Left").is_err());
        assert!(parse_key("Nope").is_err());
    }
}
