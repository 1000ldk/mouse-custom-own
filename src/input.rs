//! SendInput で任意のキーの組み合わせ (例: Ctrl+テンキー+, Win+テンキー+) を送る。
//!
//! # SendInput の仕組み
//! SendInput は「キーボードやマウスが物理的に操作された」のと同じ経路 (システムの入力キュー) に
//! イベントを差し込む API。差し込まれたイベントは本物の入力と同様にフォアグラウンドのウィンドウへ
//! 配送される。そのため送り先のウィンドウを指定する引数は無い。
//!
//! 1 回のキー押下は「押す (keydown)」と「離す (KEYEVENTF_KEYUP)」の 2 イベント。
//! INPUT 構造体の配列をまとめて渡すと、他の入力が間に割り込まずに連続して処理される。
//!
//! # 修飾キーの扱い
//! タッチパッドのピンチ中は、ドライバが Ctrl を押した状態をシミュレートしている。
//! そのため「いま押されている修飾キー」と「送りたい組み合わせの修飾キー」を比べて差分だけ操作する。
//! - 送りたい組み合わせに Ctrl がある (Ctrl+テンキー+) → Ctrl は既に押されているので触らない。
//!   ここで Ctrl を離すと、ピンチがまだ続いているのに Ctrl が離されたことになり、
//!   後続のイベントがただのスクロールになってしまう。
//! - 送りたい組み合わせに Ctrl が無い (Win+テンキー+) → そのままだと Ctrl+Win+テンキー+ になるので、
//!   一時的に Ctrl を離し、キーを送った後に押し直して元の状態に戻す。
//! - 足りない修飾キー (Win など) は押して、送った後に離す。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC_EX, MapVirtualKeyW, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

use crate::keys::KeyCombo;

/// フォアグラウンドのウィンドウにキーの組み合わせを 1 回送る。
pub fn send_combo(combo: KeyCombo) {
    // (修飾キー, 送りたい組み合わせに含まれるか, いま押されているか)
    let modifiers = [
        (VK_CONTROL, combo.ctrl, is_down(VK_CONTROL)),
        (VK_SHIFT, combo.shift, is_down(VK_SHIFT)),
        (VK_MENU, combo.alt, is_down(VK_MENU)), // VK_MENU = Alt
        (VK_LWIN, combo.win, is_down(VK_LWIN) || is_down(VK_RWIN)),
    ];

    let mut inputs = Vec::with_capacity(10);
    // 1. 修飾キーの状態を「送りたい組み合わせ」に合わせる
    for &(vk, want, held) in &modifiers {
        if want && !held {
            inputs.push(key_input(vk, false)); // 足りないものを押す
        } else if !want && held {
            inputs.push(key_input(vk, true)); // 余計なものを一時的に離す
        }
    }
    // 2. 本体のキーを押して離す
    let key = VIRTUAL_KEY(combo.vk);
    inputs.push(key_input(key, false));
    inputs.push(key_input(key, true));
    // 3. 修飾キーを元の状態に戻す (逆順)
    for &(vk, want, held) in modifiers.iter().rev() {
        if want && !held {
            inputs.push(key_input(vk, true));
        } else if !want && held {
            inputs.push(key_input(vk, false));
        }
    }

    // SAFETY: inputs は有効な INPUT 配列。第 2 引数は構造体サイズ (API のバージョン判定に使われる)。
    // 戻り値は実際に差し込めたイベント数。UIPI (README 参照) でブロックされると 0 になる。
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Ctrl が「いま」押されているか。
pub fn is_ctrl_down() -> bool {
    is_down(VK_CONTROL)
}

/// GetAsyncKeyState は呼び出した瞬間の物理的 (+注入された) キー状態を返す。
/// 戻り値の最上位ビットが 1 なら押下中。i16 として見ると負の値になる。
fn is_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    // 仮想キーコード (VK_*) は「論理的なキー」、スキャンコードは「物理的なキーの位置」。
    // Chromium / VS Code はキーバインド判定に物理位置 (KeyboardEvent.code = "NumpadAdd") を使うので、
    // 仮想キーだけでなくスキャンコードも埋めておく。
    // MAPVK_VK_TO_VSC_EX は「拡張キー」(Win キー、矢印キー、Insert/Delete など) なら 0xE0xx を返す。
    // 拡張キーは KEYEVENTF_EXTENDEDKEY を付けないと別のキー (テンキー側) と解釈される。
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC_EX) };
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if scan & 0xFF00 == 0xE000 {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        // INPUT_0 は C の union (キーボード / マウス / ハードウェアのどれか)。
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: (scan & 0xFF) as u16,
                dwFlags: flags,
                time: 0, // 0 ならシステムがタイムスタンプを付ける
                dwExtraInfo: 0,
            },
        },
    }
}
