//! Mac 版 pinch-zoom。
//!
//! トラックパッドのピンチを、ピンチしたウィンドウのアプリごとに設定した動作に変換する常駐ツール。
//! 既定では VS Code は ⌘+テンキー± でウィンドウ全体のズーム、ブラウザなどはアプリ本来のピンチ、
//! それ以外は **ピンチしたウィンドウだけを Chrome のピンチのように拡大** する。
//!
//! 全体の流れ:
//! ```text
//!  [トラックパッド] ─ピンチ→ macOS が「拡大ジェスチャー」のイベント (NSEventTypeMagnify) を作る
//!        │
//!        ▼  アプリに届く前に (メインスレッドの RunLoop で呼ばれる)
//!  tap::callback (CGEventTap)
//!        ├─ 拡大ジェスチャー: ポインタの下のウィンドウのアプリで [[rules]] を選ぶ
//!        │     ├─ pass        → そのまま通す (アプリ本来のピンチ)
//!        │     ├─ zoom_in/out → gesture::PinchTracker → input::send_combo (⌘+テンキー± などを送る)
//!        │     └─ window_zoom → zoom (ウィンドウ単位のズーム。下記)
//!        │   pass 以外は元のイベントを握りつぶす (アプリ本来のズームと二重にならない)
//!        ├─ スクロール (拡大中): 拡大表示の中で見る場所を動かす。端まで来たらアプリに渡す
//!        └─ クリック・マウス移動 (拡大中): 「見えている位置」→「本当の位置」に座標を書き換えて通す
//!
//!  zoom (ウィンドウ単位のズーム):
//!    capture: ScreenCaptureKit で対象のウィンドウだけを撮り続ける
//!        ▼ (1 フレームごと)
//!    overlay: 対象のウィンドウにぴったり重ねた、クリックを素通しする透明ウィンドウに拡大して表示
//!    計算 (倍率・表示位置・座標の変換) は OS に依存しない view_zoom.rs
//! ```
//!
//! # スレッド
//! イベントタップのコールバック、タイマー、メニュー、キャプチャのフレーム受け取りは
//! すべてメインスレッドで動く (キャプチャはフレームをメインキューに届けるよう指定している)。
//! なので状態は Windows 版と同じくスレッドローカルな RefCell に置く (state.rs)。
//!
//! # 必要な権限
//! - アクセシビリティ: イベントタップでイベントを書き換える・握りつぶす、キーを送る、ウィンドウを前面に出す
//! - 画面収録: ScreenCaptureKit でウィンドウを撮る

mod apps;
mod autostart;
mod capture;
mod input;
mod menu;
mod overlay;
mod permissions;
mod state;
mod tap;
mod window_list;
mod zoom;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSApplication, NSApplicationActivationPolicy, NSRunningApplication};
use objc2_foundation::{NSBundle, NSString};

use crate::config::Config;

pub fn run() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("pinch-zoom はメインスレッドで起動してください");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // Dock にアイコンを出さない常駐アプリにする (Info.plist の LSUIElement と同じ)
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    if another_instance_is_running() {
        return; // 既に動いているので黙って終了
    }

    let config_path = Config::default_path();
    let (config, rules) = match Config::load_or_create(&config_path) {
        Ok(loaded) => loaded,
        Err(e) => {
            show_alert(
                mtm,
                "設定ファイルを読み込めませんでした",
                &format!("{e}\n\n設定ファイルを直してから、もう一度起動してください。"),
            );
            return;
        }
    };

    state::init(state::State::new(mtm, &config, rules, config_path));
    menu::install(mtm);

    // 権限の確認。まだ許可されていなければ、システムの許可ダイアログを出す。
    // アクセシビリティは許可されるまでイベントタップを作り直し続ける (tap.rs)。
    permissions::request_accessibility();
    if !permissions::screen_capture_allowed() {
        permissions::request_screen_capture();
    }
    tap::install_when_permitted();

    // メッセージループ。終了メニューで NSApp.terminate されるまで戻らない
    app.run();
}

/// 同じアプリ (バンドル ID) が既に動いているか
fn another_instance_is_running() -> bool {
    let Some(bundle_id) = NSBundle::mainBundle().bundleIdentifier() else {
        return false; // .app にまとめずに直接実行した (開発中)
    };
    let me = NSRunningApplication::currentApplication().processIdentifier();
    NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id)
        .iter()
        .any(|app| app.processIdentifier() != me)
}

/// ダイアログでメッセージを出す (閉じるまで戻らない)。
/// 内部でメッセージループを回すので、state::with_state の中から呼ばないこと。
pub fn show_alert(mtm: MainThreadMarker, title: &str, message: &str) {
    let app = NSApplication::sharedApplication(mtm);
    // 常駐アプリは前面に出ていないので、出さないとダイアログが他のウィンドウの後ろに隠れる
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    alert.runModal();
}
