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

use crate::config::Rule;
use crate::foreground::ForegroundCache;
use crate::gesture::PinchTracker;

pub struct AppState {
    pub hwnd: HWND,
    pub enabled: bool,
    pub config_path: PathBuf,
    /// config.toml から解析済みのルール (フック内で文字列処理をしないよう読み込み時に変換済み)
    pub rules: Vec<Rule>,
    pub pinch: PinchTracker,
    /// 直前のピンチに使ったルールの番号。アプリが切り替わったら蓄積をリセットするために覚える。
    pub last_rule: Option<usize>,
    pub foreground: ForegroundCache,
}

impl AppState {
    pub fn new(hwnd: HWND, rules: Vec<Rule>, config_path: PathBuf) -> Self {
        Self {
            hwnd,
            enabled: true,
            config_path,
            rules,
            pinch: PinchTracker::new(),
            last_rule: None,
            foreground: ForegroundCache::default(),
        }
    }

    pub fn apply_rules(&mut self, rules: Vec<Rule>) {
        self.rules = rules;
        self.pinch.reset();
        self.last_rule = None;
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
