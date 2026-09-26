//! アプリ全体の状態。
//!
//! # なぜ thread_local なのか
//! フックのコールバックやウィンドウプロシージャは OS から呼ばれる `extern "system" fn` なので、
//! Swift のクロージャのように状態をキャプチャできない (引数も OS が決めている)。
//! そのためどこかグローバルな場所に状態を置く必要がある。
//!
//! このアプリでは フック・ウィンドウ・メッセージループ がすべてメインスレッド 1 本で動く
//! (低レベルフックのコールバックも「フックを登録したスレッド」で呼ばれる。hook.rs 参照)。
//! なので Mutex は不要で、スレッドローカルな RefCell で十分。
//!
//! # 借用 (borrow) を短く保つ
//! TrackPopupMenu (トレイのメニュー表示) などは内部でメッセージループを回すので、その最中に
//! フックのコールバックが呼ばれることがある。RefCell を借用したままそうした API を呼ぶと、
//! コールバック側で二重借用になって panic する。`with_state` のクロージャ内では
//! メッセージを処理しうる API を呼ばないこと。

use std::cell::RefCell;
use std::path::PathBuf;

use windows::Win32::Foundation::HWND;

use crate::config::Config;
use crate::foreground::ForegroundCache;
use crate::gesture::{PinchSettings, PinchTracker};

pub struct AppState {
    pub hwnd: HWND,
    pub enabled: bool,
    pub config: Config,
    pub config_path: PathBuf,
    /// config から変換済みのパラメータ (フック内で毎回変換しないようにキャッシュ)
    pub settings: PinchSettings,
    pub pinch: PinchTracker,
    pub foreground: ForegroundCache,
}

impl AppState {
    pub fn new(hwnd: HWND, config: Config, config_path: PathBuf) -> Self {
        Self {
            hwnd,
            enabled: true,
            settings: config.pinch_settings(),
            config,
            config_path,
            pinch: PinchTracker::new(),
            foreground: ForegroundCache::default(),
        }
    }

    pub fn apply_config(&mut self, config: Config) {
        self.settings = config.pinch_settings();
        self.config = config;
        self.pinch.reset();
        self.foreground.clear();
    }
}

thread_local! {
    static STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

pub fn init(state: AppState) {
    STATE.with(|s| *s.borrow_mut() = Some(state));
}

/// 状態にアクセスする。未初期化 or 既に借用中 (再入) なら None を返し、panic しない。
///
/// `extern "system"` 関数の中で panic すると、プロセスがそのまま異常終了する
/// (Rust の panic は FFI 境界を越えられない) ので、フック内では特に panic を避ける。
pub fn with_state<R>(f: impl FnOnce(&mut AppState) -> R) -> Option<R> {
    STATE.with(|s| {
        let mut guard = s.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}
