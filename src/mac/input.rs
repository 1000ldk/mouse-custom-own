//! キーの組み合わせ (例: ⌘+テンキー+) をアプリに送る。
//!
//! CGEventCreateKeyboardEvent でキーを押す / 離すイベントを作り、修飾キーはイベントのフラグで指定する。
//! CGEventPostToPid で「ピンチしたウィンドウのアプリ」に直接届けるので、
//! そのアプリが前面になくても届く。

use objc2_core_graphics::{CGEvent, CGEventFlags};

use crate::keys::KeyCombo;

/// テンキーのキー (kVK_ANSI_Keypad*) か。テンキーのキーにはフラグ NumericPad を付けるのが決まり
fn is_keypad(code: u16) -> bool {
    matches!(
        code,
        0x41 | 0x43 | 0x45 | 0x47 | 0x4B | 0x4C | 0x4E | 0x51..=0x59 | 0x5B | 0x5C
    )
}

pub fn send_combo(combo: KeyCombo, pid: i32) {
    let mut flags = CGEventFlags::empty();
    for (on, flag) in [
        (combo.meta, CGEventFlags::MaskCommand),
        (combo.ctrl, CGEventFlags::MaskControl),
        (combo.alt, CGEventFlags::MaskAlternate),
        (combo.shift, CGEventFlags::MaskShift),
        (is_keypad(combo.vk), CGEventFlags::MaskNumericPad),
    ] {
        if on {
            flags |= flag;
        }
    }
    for key_down in [true, false] {
        let Some(event) = CGEvent::new_keyboard_event(None, combo.vk, key_down) else {
            return;
        };
        CGEvent::set_flags(Some(&event), flags);
        CGEvent::post_to_pid(pid, Some(&event));
    }
}
