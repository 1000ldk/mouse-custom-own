//! 非表示ウィンドウ・ウィンドウプロシージャ・タスクトレイ。
//!
//! # なぜ見えないウィンドウが要るのか
//! Win32 では「メッセージの受け取り手」はウィンドウ (HWND) 単位。
//! - タスクトレイのアイコンがクリックされると、Shell は登録された HWND にメッセージを送ってくる
//! - フックから「ズームして」と依頼するのにも PostMessage の宛先 HWND が要る
//!
//! そこで ShowWindow しない (= 表示されない) ウィンドウを 1 つ作り、受付窓口として使う。
//!
//! HWND_MESSAGE を親にした「メッセージ専用ウィンドウ」でも大半は動くが、それだと
//! Explorer 再起動時にブロードキャストされる "TaskbarCreated" を受け取れず、
//! トレイアイコンが消えたままになる。なので普通のトップレベルウィンドウを非表示で作る。
//!
//! # ウィンドウプロシージャ (WndProc)
//! ウィンドウに届いたメッセージを処理する関数。メッセージループの DispatchMessage が
//! 宛先ウィンドウの WndProc を呼び出す。処理しないメッセージは DefWindowProcW に任せる。

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, HICON, IDI_APPLICATION, IDI_WARNING, LoadIconW, MB_ICONERROR, MB_OK, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MessageBoxW, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SW_SHOWNORMAL, SetForegroundWindow, TPM_BOTTOMALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WM_APP, WM_CONTEXTMENU,
    WM_DESTROY, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::app::with_state;
use crate::autostart;
use crate::config::Config;
use crate::input::send_combo;
use crate::keys::KeyCombo;

/// フック → ウィンドウ: 「このキーを送って」。wParam = KeyCombo::to_bits() の値。
///
/// WM_APP 〜 0xBFFF はアプリが自由に使ってよいメッセージ番号の範囲。
pub const WM_APP_SEND_KEYS: u32 = WM_APP + 1;
/// Shell → ウィンドウ: トレイアイコンがクリックされた等。lParam にマウスメッセージが入る。
const WM_APP_TRAY: u32 = WM_APP + 2;

const TRAY_ICON_ID: u32 = 1;

// メニュー項目の ID
const ID_TOGGLE: usize = 1;
const ID_OPEN_CONFIG: usize = 2;
const ID_RELOAD_CONFIG: usize = 3;
const ID_EXIT: usize = 4;
const ID_AUTOSTART: usize = 5;

const APP_TITLE: PCWSTR = w!("pinch-zoom");

/// 非表示ウィンドウを作る。
pub fn create_hidden_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        // 「ウィンドウクラス」= WndProc などを束ねたひな形。先に登録してから、その名前でウィンドウを作る。
        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: w!("PinchZoomHiddenWindow"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class.lpszClassName,
            APP_TITLE,
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

/// エラーをダイアログで表示する (コンソールが無いので println! では見えない)。
pub fn show_error(message: &str) {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            APP_TITLE,
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Explorer が再起動したときにブロードキャストされるメッセージの番号。
/// 文字列から実行時に番号が決まる (同じ文字列なら同じ番号) ので、初回だけ問い合わせて覚えておく。
fn taskbar_created_msg() -> u32 {
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SEND_KEYS => {
            // フックのコールバックはもう戻っているので、ここでは時間を気にせず SendInput してよい。
            send_combo(KeyCombo::from_bits(wparam.0));
            LRESULT(0)
        }
        WM_APP_TRAY => {
            // 既定 (NOTIFYICON_VERSION 未指定) では lParam の値そのものがマウスメッセージ。
            match lparam.0 as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                WM_LBUTTONDBLCLK => toggle_enabled(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            tray_delete(hwnd);
            // メッセージキューに WM_QUIT を積む → main の GetMessage が 0 を返してループを抜ける
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        m if m == taskbar_created_msg() => {
            tray_add(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ---------------------------------------------------------------------------
// タスクトレイ
// ---------------------------------------------------------------------------

fn notify_icon_data(hwnd: HWND, enabled: bool) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: tray_icon(enabled),
        ..Default::default()
    };
    let tip = if enabled {
        "pinch-zoom (有効)"
    } else {
        "pinch-zoom (無効)"
    };
    // szTip は固定長の UTF-16 配列 (末尾は NUL 終端)
    for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16()) {
        *dst = src;
    }
    nid
}

/// システム標準のアイコンを使う (独自アイコンにするならリソースとして exe に埋め込む)。
fn tray_icon(enabled: bool) -> HICON {
    let id = if enabled {
        IDI_APPLICATION
    } else {
        IDI_WARNING
    };
    unsafe { LoadIconW(None, id).unwrap_or_default() }
}

pub fn tray_add(hwnd: HWND) {
    let enabled = with_state(|app| app.enabled).unwrap_or(true);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &notify_icon_data(hwnd, enabled));
    }
}

fn tray_update(hwnd: HWND, enabled: bool) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &notify_icon_data(hwnd, enabled));
    }
}

fn tray_delete(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &notify_icon_data(hwnd, false));
    }
}

fn toggle_enabled(hwnd: HWND) {
    let enabled = with_state(|app| {
        app.enabled = !app.enabled;
        app.pinch.reset();
        app.enabled
    });
    if let Some(enabled) = enabled {
        tray_update(hwnd, enabled);
    }
}

fn show_menu(hwnd: HWND) {
    let enabled = with_state(|app| app.enabled).unwrap_or(true);

    let chosen = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = if enabled { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | check, ID_TOGGLE, w!("有効"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN_CONFIG, w!("設定ファイルを開く"));
        let _ = AppendMenuW(menu, MF_STRING, ID_RELOAD_CONFIG, w!("設定を再読み込み"));
        let autostart = if autostart::is_enabled() {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let _ = AppendMenuW(
            menu,
            MF_STRING | autostart,
            ID_AUTOSTART,
            w!("Windows 起動時に自動実行"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("終了"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);

        // トレイメニューの定番のおまじない (Microsoft の既知の問題):
        // 表示前に自分のウィンドウを前面にしないと、メニュー外をクリックしても閉じない。
        // 表示後に WM_NULL を投げないと、2 回目の表示ですぐ閉じることがある。
        let _ = SetForegroundWindow(hwnd);
        // TrackPopupMenu は選ばれるまで戻らない (内部でメッセージループを回している)。
        // TPM_RETURNCMD を付けると、WM_COMMAND を送る代わりに選ばれた ID を戻り値で返す。
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    };

    match chosen {
        ID_TOGGLE => toggle_enabled(hwnd),
        ID_OPEN_CONFIG => open_config(),
        ID_RELOAD_CONFIG => reload_config(),
        ID_AUTOSTART => {
            if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                show_error(&format!("自動実行の設定に失敗しました。\n\n{e}"));
            }
        }
        ID_EXIT => unsafe {
            // WM_DESTROY → PostQuitMessage → ループ終了、の順で後片付けが走る
            let _ = DestroyWindow(hwnd);
        },
        _ => {} // メニューがキャンセルされた (0)
    }
}

fn open_config() {
    let Some(path) = with_state(|app| app.config_path.clone()) else {
        return;
    };
    unsafe {
        // .toml に関連付けが無い環境もあるので、メモ帳を明示して開く
        ShellExecuteW(
            None,
            w!("open"),
            w!("notepad.exe"),
            &HSTRING::from(format!("\"{}\"", path.display())),
            None,
            SW_SHOWNORMAL,
        );
    }
}

fn reload_config() {
    let Some(path) = with_state(|app| app.config_path.clone()) else {
        return;
    };
    // MessageBox もメッセージループを回すので、借用の外で呼ぶ
    match Config::load_or_create(&path) {
        Ok((_, rules)) => {
            with_state(|app| app.apply_rules(rules));
        }
        Err(e) => show_error(&format!("設定ファイルを読み込めませんでした。\n\n{e}")),
    }
}
