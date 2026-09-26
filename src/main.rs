//! pinch-zoom: タッチパッドのピンチ (= Ctrl+ホイール) を、VS Code のウィンドウズーム
//! (Ctrl+テンキー+ / Ctrl+テンキー-) に変換する常駐ツール。
//!
//! 全体の流れ:
//! ```text
//!  [タッチパッド] ─ピンチ→ Ctrl+WM_MOUSEWHEEL
//!        │
//!        ▼  (OS がメインスレッドのメッセージ待ちに割り込んで呼ぶ)
//!  hook::mouse_proc ── 対象外 ──→ CallNextHookEx (そのまま通す)
//!        │ 対象アプリ (Code.exe 等) がフォアグラウンド
//!        ├─ gesture::PinchTracker で delta を蓄積、閾値超え & クールダウン外なら
//!        │     PostMessage(WM_APP_ZOOM) で自分のウィンドウに依頼
//!        └─ LRESULT(1) を返して元のイベントを握りつぶす
//!
//!  メッセージループ (main) ─ GetMessage → DispatchMessage
//!        ▼
//!  window::wnd_proc ── WM_APP_ZOOM → input::send_zoom (SendInput で Ctrl+テンキー±)
//!                   └─ WM_APP_TRAY → トレイメニュー (有効/無効, 設定, 終了)
//! ```

// release ビルドでは「Windows サブシステム」の exe にする = 起動してもコンソール窓が出ない。
// debug ビルドはコンソールを残し、eprintln! などでデバッグできるようにしている。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod gesture;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod foreground;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod window;

#[cfg(not(windows))]
fn main() {
    eprintln!("pinch-zoom は Windows 専用です (ロジックのテストは `cargo test` で実行できます)。");
}

#[cfg(windows)]
fn main() {
    if let Err(message) = run() {
        window::show_error(&message);
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, TranslateMessage,
    };
    use windows::core::w;

    // --- 多重起動防止 ------------------------------------------------------------
    // 名前付きミューテックスはセッション内で共有される。既に同名のものがあれば 2 つ目の起動。
    let mutex = unsafe { CreateMutexW(None, false, w!("Local\\pinch-zoom-single-instance")) }
        .map_err(|e| format!("CreateMutexW failed: {e}"))?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return Ok(()); // 既に動いているので黙って終了
    }

    // --- 設定 --------------------------------------------------------------------
    let config_path = config::Config::default_path();
    let config = config::Config::load_or_create(&config_path)
        .map_err(|e| format!("設定ファイルを読み込めませんでした。\n\n{e}"))?;

    // --- ウィンドウ・状態・トレイ・フック -------------------------------------------
    let hwnd = window::create_hidden_window().map_err(|e| format!("ウィンドウ作成に失敗: {e}"))?;
    app::init(app::AppState::new(hwnd, config, config_path));
    window::tray_add(hwnd);

    // フックは「このスレッド」に紐づく。この後のメッセージループが回っている間だけ呼ばれる。
    let _hook = hook::MouseHook::install().map_err(|e| format!("マウスフックの登録に失敗: {e}"))?;

    // --- メッセージループ -----------------------------------------------------------
    // GetMessage はこのスレッド宛てのメッセージが来るまでブロックする (CPU は使わない)。
    // 待っている間に OS はこのスレッドで低レベルフックのコールバックを実行する。
    // メッセージを取り出したら DispatchMessage が宛先ウィンドウの WndProc を呼ぶ。
    // WM_QUIT を受け取ると GetMessage が 0 を返すのでループを抜ける (-1 はエラー)。
    let mut msg = MSG::default();
    loop {
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if ret.0 == 0 || ret.0 == -1 {
            break;
        }
        unsafe {
            // キー押下メッセージから文字メッセージ (WM_CHAR) を作る。定型句。
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // _hook がここで Drop → UnhookWindowsHookEx
    unsafe {
        let _ = CloseHandle(mutex);
    }
    Ok(())
}
