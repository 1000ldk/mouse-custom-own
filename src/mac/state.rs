//! Mac 版の状態。
//!
//! Windows 版の app.rs と同じ考え方: イベントタップのコールバックやメニューの処理は OS から呼ばれる
//! 関数なので、状態はグローバルな場所に置くしかない。すべてメインスレッドで動くので、
//! スレッドローカルな RefCell で足りる。
//!
//! # 借用を短く保つ
//! ダイアログ (NSAlert) やメニューの表示は内部でメッセージループを回すので、その最中にも
//! イベントタップのコールバックが呼ばれる。`with_state` のクロージャの中でそうした処理をすると
//! 二重借用になる (with_state は panic せず None を返す) ので、クロージャの外で行うこと。

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2_core_foundation::{CFMachPort, CFRetained};

use super::overlay::Overlay;
use super::tap::Target;
use super::zoom::Session;
use crate::config::{Config, Rule};
use crate::gesture::PinchTracker;
use crate::screen_zoom::ScreenZoomSettings;

pub struct State {
    pub mtm: MainThreadMarker,
    pub enabled: bool,
    pub config_path: PathBuf,
    /// config.toml から解析済みのルール
    pub rules: Vec<Rule>,
    /// キーを送るルール用の、ピンチ量の蓄積
    pub pinch: PinchTracker,
    /// 直前のピンチに使ったルールの番号 (アプリが変わったら蓄積をリセットする)
    pub last_rule: Option<usize>,
    pub zoom_settings: ScreenZoomSettings,
    /// いま続いているピンチの行き先。ピンチの始まりで決め、終わるまで同じものを使う
    pub gesture: Option<Target>,
    /// ウィンドウズーム中の状態 (拡大していなければ None)
    pub session: Option<Session>,
    /// 拡大表示用のウィンドウ。初めて使うときに作り、使い回す
    pub overlay: Option<Overlay>,
    pub next_session_id: u64,
    /// イベントタップ。無効化されたときに有効に戻すため持っておく
    pub tap: Option<CFRetained<CFMachPort>>,
    /// 自分のプロセス ID (自分のウィンドウをピンチの対象から外すため)
    pub own_pid: i32,
    /// 画面収録の許可が無いことを既に伝えたか (何度もダイアログを出さない)
    pub capture_error_shown: bool,
}

impl State {
    pub fn new(mtm: MainThreadMarker, config: &Config, rules: Vec<Rule>, path: PathBuf) -> Self {
        Self {
            mtm,
            enabled: true,
            config_path: path,
            rules,
            pinch: PinchTracker::new(),
            last_rule: None,
            zoom_settings: config.screen_zoom_settings(),
            gesture: None,
            session: None,
            overlay: None,
            next_session_id: 1,
            tap: None,
            own_pid: std::process::id() as i32,
            capture_error_shown: false,
        }
    }

    pub fn apply_config(&mut self, config: &Config, rules: Vec<Rule>) {
        self.zoom_settings = config.screen_zoom_settings();
        self.rules = rules;
        self.pinch.reset();
        self.last_rule = None;
        self.gesture = None;
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn init(state: State) {
    STATE.with(|s| *s.borrow_mut() = Some(state));
}

/// 状態にアクセスする。未初期化 or 既に借用中 (再入) なら None を返し、panic しない。
/// (OS から呼ばれるコールバックの中で panic するとプロセスごと落ちるため)
pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| {
        let mut guard = s.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}
