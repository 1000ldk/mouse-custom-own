//! "Ctrl+NumpadAdd" や "Win+Plus" のようなキーの組み合わせ文字列を解析する。
//!
//! Win32 に依存しない (仮想キーコードは数値で持つ) ので Linux でもテストできる。
//! 仮想キーコードの一覧: https://learn.microsoft.com/windows/win32/inputdev/virtual-key-codes

/// 修飾キー + 1 つのキー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    /// 仮想キーコード (VK_*)
    pub vk: u16,
}

impl KeyCombo {
    /// `"Ctrl+Shift+NumpadAdd"` のような文字列を解析する。大文字小文字は区別しない。
    pub fn parse(text: &str) -> Result<KeyCombo, String> {
        let mut combo = KeyCombo {
            ctrl: false,
            shift: false,
            alt: false,
            win: false,
            vk: 0,
        };
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        // "Ctrl++" のように + キー自体を書いた場合、最後の 2 要素が空文字になる
        let (mods, key) = match parts.as_slice() {
            [mods @ .., "", ""] => (mods, "+"),
            [mods @ .., key] => (mods, *key),
            [] => return Err(format!("キーが空です: \"{text}\"")),
        };
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => combo.ctrl = true,
                "shift" => combo.shift = true,
                "alt" => combo.alt = true,
                "win" | "windows" => combo.win = true,
                _ => return Err(format!("不明な修飾キー \"{m}\" (\"{text}\")")),
            }
        }
        combo.vk =
            key_name_to_vk(key).ok_or_else(|| format!("不明なキー \"{key}\" (\"{text}\")"))?;
        Ok(combo)
    }

    /// PostMessage の WPARAM (整数 1 つ) で運べるように詰める。
    /// 下位 16bit = 仮想キー, bit16〜19 = Ctrl / Shift / Alt / Win
    pub fn to_bits(self) -> usize {
        self.vk as usize
            | (self.ctrl as usize) << 16
            | (self.shift as usize) << 17
            | (self.alt as usize) << 18
            | (self.win as usize) << 19
    }

    pub fn from_bits(bits: usize) -> KeyCombo {
        KeyCombo {
            vk: bits as u16,
            ctrl: bits & (1 << 16) != 0,
            shift: bits & (1 << 17) != 0,
            alt: bits & (1 << 18) != 0,
            win: bits & (1 << 19) != 0,
        }
    }
}

fn key_name_to_vk(name: &str) -> Option<u16> {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();

    // 1 文字の英数字: 'A'〜'Z' と '0'〜'9' は VK コードが ASCII コードと同じ
    if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() {
        return Some(bytes[0].to_ascii_uppercase() as u16);
    }
    // F1〜F24
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        return (1..=24).contains(&n).then_some(0x70 + n - 1);
    }
    // Numpad0〜Numpad9
    if let Some(n) = lower
        .strip_prefix("numpad")
        .and_then(|n| n.parse::<u16>().ok())
    {
        return (n <= 9).then_some(0x60 + n);
    }
    // 数値で直接指定: "vk:0xBB" / "vk:187"
    if let Some(v) = lower.strip_prefix("vk:") {
        return match v.strip_prefix("0x") {
            Some(hex) => u16::from_str_radix(hex, 16).ok(),
            None => v.parse().ok(),
        };
    }

    let vk = match lower.as_str() {
        "numpadadd" => 0x6B,
        "numpadsubtract" | "numpadsub" => 0x6D,
        "numpadmultiply" => 0x6A,
        "numpaddivide" => 0x6F,
        "numpaddecimal" => 0x6E,
        // メインキーボードの ; + (JIS) / = + (US) の位置と - の位置
        "plus" | "+" | "=" => 0xBB, // VK_OEM_PLUS
        "minus" | "-" => 0xBD,      // VK_OEM_MINUS
        "space" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "insert" => 0x2D,
        "delete" | "del" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        _ => return None,
    };
    Some(vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_key() {
        let c = KeyCombo::parse("Ctrl+NumpadAdd").unwrap();
        assert!(c.ctrl && !c.shift && !c.alt && !c.win);
        assert_eq!(c.vk, 0x6B);

        let c = KeyCombo::parse("win + shift + numpadsubtract").unwrap();
        assert!(c.win && c.shift && !c.ctrl);
        assert_eq!(c.vk, 0x6D);
    }

    #[test]
    fn parses_various_keys() {
        assert_eq!(KeyCombo::parse("Ctrl+A").unwrap().vk, b'A' as u16);
        assert_eq!(KeyCombo::parse("Ctrl+0").unwrap().vk, b'0' as u16);
        assert_eq!(KeyCombo::parse("F12").unwrap().vk, 0x7B);
        assert_eq!(KeyCombo::parse("Ctrl+Numpad0").unwrap().vk, 0x60);
        assert_eq!(KeyCombo::parse("Ctrl+Plus").unwrap().vk, 0xBB);
        assert_eq!(KeyCombo::parse("Ctrl++").unwrap().vk, 0xBB);
        assert_eq!(KeyCombo::parse("Ctrl+vk:0xBB").unwrap().vk, 0xBB);
    }

    #[test]
    fn bits_round_trip() {
        for text in [
            "Ctrl+NumpadAdd",
            "Win+Shift+Minus",
            "Alt+F4",
            "Ctrl+Shift+Alt+Win+Z",
        ] {
            let c = KeyCombo::parse(text).unwrap();
            assert_eq!(KeyCombo::from_bits(c.to_bits()), c);
        }
    }

    #[test]
    fn rejects_unknown() {
        assert!(KeyCombo::parse("Hyper+A").is_err());
        assert!(KeyCombo::parse("Ctrl+Nope").is_err());
        assert!(KeyCombo::parse("Ctrl+F25").is_err());
    }
}
