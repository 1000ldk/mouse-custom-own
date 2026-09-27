//! pinch-zoom: タッチパッドのピンチ (= Ctrl+ホイール) を、アプリごとに設定した任意のキー
//! (VS Code なら Ctrl+テンキー± でウィンドウズーム、それ以外は Win+テンキー± で拡大鏡など) に変換する常駐ツール。
//!
//! 全体の流れ:
//! ```text
//!  [タッチパッド] ─ピンチ→ Ctrl+WM_MOUSEWHEEL
//!        │
//!        ▼  (OS がメインスレッドのメッセージ待ちに割り込んで呼ぶ)
//!  [タッチパッド] ─生データ→ WM_INPUT → touchpad (2 本指の間隔の変化からピンチを検出)
//!        └─ フックと同じルールで処理 (Ctrl+ホイールを受け取れないアプリでも動くようにするため)
//!
//!  hook::mouse_proc ── 一致するルール無し / pass ルール ──→ CallNextHookEx (そのまま通す)
//!        │ フォアグラウンドのアプリに一致するルールがある (config.toml の [[rules]])
//!        ├─ gesture::PinchTracker で delta を蓄積、閾値超え & クールダウン外なら
//!        │     ルールのキー (例: Ctrl+NumpadAdd) を PostMessage(WM_APP_SEND_KEYS) で自分のウィンドウに依頼
//!        └─ LRESULT(1) を返して元のイベントを握りつぶす
//!
//!  メッセージループ (main) ─ GetMessage → DispatchMessage
//!        ▼
//!  window::wnd_proc ── WM_APP_SEND_KEYS → input::send_combo (SendInput でキーを送る)
//!                   ├─ WM_APP_UPDATE_SCREEN_ZOOM → magnifier (画面全体を連続ズーム。screen_zoom ルール)
//!                   └─ WM_APP_TRAY → トレイメニュー (有効/無効, 設定, 終了)
//! ```

// release ビルドでは「Windows サブシステム」の exe にする = 起動してもコンソール窓が出ない。
// debug ビルドはコンソールを残し、eprintln! などでデバッグできるようにしている。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod gesture;
mod keys;
mod screen_zoom;
mod touch_pinch;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod autostart;
#[cfg(windows)]
mod foreground;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod input;
#[cfg(windows)]
mod magnifier;
#[cfg(windows)]
mod touchpad;
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
    let (config, rules) = config::Config::load_or_create(&config_path)
        .map_err(|e| format!("設定ファイルを読み込めませんでした。\n\n{e}"))?;

    // --- ウィンドウ・状態・トレイ・フック -------------------------------------------
    // 画面ズーム用の Magnification API を初期化。失敗しても (screen_zoom ルールが素通しになるだけで) 動作は続ける。
    // 変数を _hook より先に作るので、終了時は フック解除 → 倍率を 1 倍に戻す の順に後片付けされる。
    let magnifier = magnifier::Magnifier::init();

    let hwnd = window::create_hidden_window().map_err(|e| format!("ウィンドウ作成に失敗: {e}"))?;
    app::init(app::AppState::new(
        hwnd,
        &config,
        rules,
        config_path,
        magnifier.is_some(),
    ));
    window::tray_add(hwnd);

    // タッチパッドの生データを受け取る。Ctrl+ホイールを受け取れないアプリ (Chrome, エクスプローラー等) でも
    // ピンチを検出するため。失敗してもマウスフック経由の従来の動作は続ける。
    let _ = touchpad::register(hwnd);

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
