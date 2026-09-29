//! "Ctrl+NumpadAdd" や "Cmd+Minus" のようなキーの組み合わせ文字列を解析する。
//!
//! OS の API には依存しない (キーコードは数値で持つ) ので Linux でもテストできる。
//! キーコードは OS ごとに体系が違う:
//! - Windows: 仮想キーコード (VK_*) https://learn.microsoft.com/windows/win32/inputdev/virtual-key-codes
//! - Mac: キーコード (kVK_*。HIToolbox/Events.h)。キーボード上の「位置」を表す番号

use crate::platform::Platform;

/// 修飾キー + 1 つのキー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    /// Alt (Mac では Option)
    pub alt: bool,
    /// Windows キー (Mac では Command ⌘)
    pub meta: bool,
    /// キーコード (Windows は VK_*, Mac は kVK_*)
    pub vk: u16,
}

impl KeyCombo {
    /// `"Ctrl+Shift+NumpadAdd"` のような文字列を、指定した OS のキーコードで解析する。
    /// 大文字小文字は区別しない。
    pub fn parse_for(text: &str, platform: Platform) -> Result<KeyCombo, String> {
        let mut combo = KeyCombo {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
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
                "alt" | "option" | "opt" => combo.alt = true,
                "win" | "windows" | "cmd" | "command" | "meta" => combo.meta = true,
                _ => return Err(format!("不明な修飾キー \"{m}\" (\"{text}\")")),
            }
        }
        let code = match platform {
            Platform::Windows => windows_vk(key),
            Platform::Mac => mac_key_code(key),
        };
        combo.vk = code.ok_or_else(|| format!("不明なキー \"{key}\" (\"{text}\")"))?;
        Ok(combo)
    }

    /// PostMessage の WPARAM (整数 1 つ) で運べるように詰める (Windows 版)。
    /// 下位 16bit = キーコード, bit16〜19 = Ctrl / Shift / Alt / Win
    #[cfg(any(test, windows))]
    pub fn to_bits(self) -> usize {
        self.vk as usize
            | (self.ctrl as usize) << 16
            | (self.shift as usize) << 17
            | (self.alt as usize) << 18
            | (self.meta as usize) << 19
    }

    #[cfg(any(test, windows))]
    pub fn from_bits(bits: usize) -> KeyCombo {
        KeyCombo {
            vk: bits as u16,
            ctrl: bits & (1 << 16) != 0,
            shift: bits & (1 << 17) != 0,
            alt: bits & (1 << 18) != 0,
            meta: bits & (1 << 19) != 0,
        }
    }
}

/// 数値で直接指定: "vk:0xBB" / "vk:187"
fn raw_code(lower: &str) -> Option<Option<u16>> {
    let v = lower.strip_prefix("vk:")?;
    Some(match v.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => v.parse().ok(),
    })
}

fn windows_vk(name: &str) -> Option<u16> {
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
    if let Some(code) = raw_code(&lower) {
        return code;
    }

    let vk = match lower.as_str() {
        "numpadadd" => 0x6B,
        "numpadsubtract" | "numpadsub" => 0x6D,
        "numpadmultiply" => 0x6A,
        "numpaddivide" => 0x6F,
        "numpaddecimal" => 0x6E,
        // メインキーボードの ; + (JIS) / = + (US) の位置と - の位置
        "plus" | "+" | "=" | "equal" => 0xBB, // VK_OEM_PLUS
        "minus" | "-" => 0xBD,                // VK_OEM_MINUS
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

/// Mac のキーコード (kVK_*)。英字や数字も「US 配列でその文字があるキーの位置」の番号で、
/// ASCII コードとは無関係なので表で引く。
fn mac_key_code(name: &str) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, // A〜M
        0x2D, 0x1F, 0x23, 0x0C, 0x0F, 0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06, // N〜Z
    ];
    const DIGITS: [u16; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
    const NUMPAD_DIGITS: [u16; 10] = [0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5B, 0x5C];
    const F_KEYS: [u16; 20] = [
        0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, // F1〜F10
        0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A, 0x40, 0x4F, 0x50, 0x5A, // F11〜F20
    ];

    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();

    if bytes.len() == 1 && bytes[0].is_ascii_lowercase() {
        return Some(LETTERS[(bytes[0] - b'a') as usize]);
    }
    if bytes.len() == 1 && bytes[0].is_ascii_digit() {
        return Some(DIGITS[(bytes[0] - b'0') as usize]);
    }
    // F1〜F20 (Mac には F21 以降が無い)
    if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<usize>().ok())
    {
        return F_KEYS.get(n.checked_sub(1)?).copied();
    }
    if let Some(n) = lower
        .strip_prefix("numpad")
        .and_then(|n| n.parse::<usize>().ok())
    {
        return NUMPAD_DIGITS.get(n).copied();
    }
    if let Some(code) = raw_code(&lower) {
        return code;
    }

    let code = match lower.as_str() {
        "numpadadd" => 0x45,
        "numpadsubtract" | "numpadsub" => 0x4E,
        "numpadmultiply" => 0x43,
        "numpaddivide" => 0x4B,
        "numpaddecimal" => 0x41,
        // US 配列の = + の位置 (JIS 配列では ^ の位置) と - の位置
        "plus" | "+" | "=" | "equal" => 0x18,
        "minus" | "-" => 0x1B,
        "space" => 0x31,
        "enter" | "return" => 0x24,
        "tab" => 0x30,
        "esc" | "escape" => 0x35,
        "backspace" => 0x33,      // Mac のキーボードの「delete」
        "delete" | "del" => 0x75, // 前方削除 (fn+delete)
        "home" => 0x73,
        "end" => 0x77,
        "pageup" => 0x74,
        "pagedown" => 0x79,
        "left" => 0x7B,
        "right" => 0x7C,
        "down" => 0x7D,
        "up" => 0x7E,
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(text: &str) -> KeyCombo {
        KeyCombo::parse_for(text, Platform::Windows).unwrap()
    }

    fn mac(text: &str) -> KeyCombo {
        KeyCombo::parse_for(text, Platform::Mac).unwrap()
    }

    #[test]
    fn parses_modifiers_and_key() {
        let c = win("Ctrl+NumpadAdd");
        assert!(c.ctrl && !c.shift && !c.alt && !c.meta);
        assert_eq!(c.vk, 0x6B);

        let c = win("win + shift + numpadsubtract");
        assert!(c.meta && c.shift && !c.ctrl);
        assert_eq!(c.vk, 0x6D);

        // Mac の修飾キーの呼び方
        let c = mac("Cmd+Option+NumpadAdd");
        assert!(c.meta && c.alt && !c.ctrl && !c.shift);
        assert_eq!(c.vk, 0x45);
    }

    #[test]
    fn parses_various_windows_keys() {
        assert_eq!(win("Ctrl+A").vk, b'A' as u16);
        assert_eq!(win("Ctrl+0").vk, b'0' as u16);
        assert_eq!(win("F12").vk, 0x7B);
        assert_eq!(win("Ctrl+Numpad0").vk, 0x60);
        assert_eq!(win("Ctrl+Plus").vk, 0xBB);
        assert_eq!(win("Ctrl++").vk, 0xBB);
        assert_eq!(win("Ctrl+vk:0xBB").vk, 0xBB);
    }

    #[test]
    fn parses_various_mac_keys() {
        assert_eq!(mac("Cmd+A").vk, 0x00);
        assert_eq!(mac("Cmd+Z").vk, 0x06);
        assert_eq!(mac("Cmd+0").vk, 0x1D);
        assert_eq!(mac("Cmd+9").vk, 0x19);
        assert_eq!(mac("F1").vk, 0x7A);
        assert_eq!(mac("F12").vk, 0x6F);
        assert_eq!(mac("Cmd+Numpad8").vk, 0x5B);
        assert_eq!(mac("Cmd+NumpadSubtract").vk, 0x4E);
        assert_eq!(mac("Cmd+Plus").vk, 0x18);
        assert_eq!(mac("Cmd+=").vk, 0x18);
        assert_eq!(mac("Cmd+Minus").vk, 0x1B);
        assert_eq!(mac("Cmd+vk:0x18").vk, 0x18);
        assert!(KeyCombo::parse_for("F21", Platform::Mac).is_err());
        assert!(KeyCombo::parse_for("F0", Platform::Mac).is_err());
        assert!(KeyCombo::parse_for("Numpad10", Platform::Mac).is_err());
    }

    #[test]
    fn bits_round_trip() {
        for text in [
            "Ctrl+NumpadAdd",
            "Win+Shift+Minus",
            "Alt+F4",
            "Ctrl+Shift+Alt+Win+Z",
        ] {
            let c = win(text);
            assert_eq!(KeyCombo::from_bits(c.to_bits()), c);
        }
    }

    #[test]
    fn rejects_unknown() {
        for platform in [Platform::Windows, Platform::Mac] {
            assert!(KeyCombo::parse_for("Hyper+A", platform).is_err());
            assert!(KeyCombo::parse_for("Ctrl+Nope", platform).is_err());
            assert!(KeyCombo::parse_for("Ctrl+F25", platform).is_err());
        }
    }
}
