//! SendInput でズーム用のキー入力を送る。
//!
//! # SendInput の仕組み
//! SendInput は「キーボードやマウスが物理的に操作された」のと同じ経路 (システムの入力キュー) に
//! イベントを差し込む API。差し込まれたイベントは本物の入力と同様にフォアグラウンドのウィンドウへ
//! 配送される。そのため送り先のウィンドウを指定する引数は無い。
//!
//! 1 回のキー押下は「押す (keydown)」と「離す (KEYEVENTF_KEYUP)」の 2 イベント。
//! INPUT 構造体の配列をまとめて渡すと、他の入力が間に割り込まずに連続して処理される。
//!
//! # Ctrl の扱い
//! タッチパッドのピンチ中は、ドライバが Ctrl を押した状態をシミュレートしている。
//! - Ctrl が既に押されている: テンキー+ だけ送れば Ctrl+テンキー+ になる。
//!   ここで Ctrl の keyup まで送ると、ピンチがまだ続いているのに Ctrl が離されたことになり、
//!   後続のイベントがただのスクロールになってしまう。
//! - Ctrl が押されていない (ピンチ終了直後など): Ctrl down → キー → Ctrl up を自前で送る。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_ADD, VK_CONTROL,
    VK_SUBTRACT,
};

use crate::gesture::ZoomDirection;

/// フォアグラウンドのウィンドウに Ctrl+テンキー+ / Ctrl+テンキー- を送る。
pub fn send_zoom(direction: ZoomDirection) {
    let key = match direction {
        ZoomDirection::In => VK_ADD,       // テンキーの +
        ZoomDirection::Out => VK_SUBTRACT, // テンキーの -
    };

    let mut inputs = Vec::with_capacity(4);
    let need_ctrl = !is_ctrl_down();
    if need_ctrl {
        inputs.push(key_input(VK_CONTROL, false));
    }
    inputs.push(key_input(key, false));
    inputs.push(key_input(key, true));
    if need_ctrl {
        inputs.push(key_input(VK_CONTROL, true));
    }

    // SAFETY: inputs は有効な INPUT 配列。第 2 引数は構造体サイズ (API のバージョン判定に使われる)。
    // 戻り値は実際に差し込めたイベント数。UIPI (後述の README 参照) でブロックされると 0 になる。
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Ctrl が「いま」押されているか。
///
/// GetAsyncKeyState は呼び出した瞬間の物理的 (+注入された) キー状態を返す。
/// 戻り値の最上位ビットが 1 なら押下中。i16 として見ると負の値になる。
pub fn is_ctrl_down() -> bool {
    unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 }
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    // 仮想キーコード (VK_*) は「論理的なキー」、スキャンコードは「物理的なキーの位置」。
    // Chromium / VS Code はキーバインド判定に物理位置 (KeyboardEvent.code = "NumpadAdd") を使うので、
    // 仮想キーだけでなくスキャンコードも埋めておく。
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        // INPUT_0 は C の union (キーボード / マウス / ハードウェアのどれか)。
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0, // 0 ならシステムがタイムスタンプを付ける
                dwExtraInfo: 0,
            },
        },
    }
}
