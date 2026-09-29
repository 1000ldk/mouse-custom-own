//! ウィンドウ単位のズーム (Chrome のピンチのような拡大)。拡大を始めてから等倍に戻すまでを
//! 1 つの「セッション」として扱う。
//!
//! - 始める: 対象のウィンドウを前面に出し、撮影 (capture.rs) を始め、拡大表示 (overlay.rs) を重ねる。
//!   最初のフレームが届くまでは拡大表示を出さない (それまでは本物のウィンドウがそのまま見えている)
//! - 続ける: ピンチで倍率、スクロールで表示位置を変える (計算は view_zoom.rs)
//! - 終わる: 等倍に戻したとき、ウィンドウの外をクリックしたとき、別のアプリに切り替えたとき、
//!   ウィンドウが閉じられた / 隠れたとき、ダイアログなどが上に開いたとき
//!
//! ウィンドウが動いた・大きさが変わった・閉じられたなどは通知が来ないので、
//! 拡大中だけ 0.1 秒ごとのタイマーで調べる。

use std::ptr::NonNull;
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_foundation::NSTimer;

use super::apps;
use super::capture::{self, Capture};
use super::overlay::Overlay;
use super::state::{State, with_state};
use super::window_list::{self, WindowInfo};
use crate::view_zoom::ViewZoom;

/// 拡大中にウィンドウの状態を調べる間隔 (秒)
const TICK_SECONDS: f64 = 0.1;
/// 始めた直後は、対象のアプリを前面に出す処理が終わるのを待ってから「別のアプリに切り替わった」を調べる
const GRACE: Duration = Duration::from_millis(500);

pub struct Session {
    pub id: u64,
    pub window: WindowInfo,
    pub view: ViewZoom,
    capture: Option<Capture>,
    /// 最初のフレームが届いて、拡大表示が出ているか
    showing: bool,
    /// 拡大表示より前面にあるウィンドウ (Dock、メニューバー、メニューなど) の範囲。
    /// そこはクリックの座標を書き換えない
    above: Vec<CGRect>,
    /// 拡大表示の上でボタンを押した。離すまでは外に出ても座標を書き換え続ける (ドラッグ)
    pub dragging: bool,
    timer: Retained<NSTimer>,
    started: Instant,
    /// 前面にあるアプリ。これが別のアプリに変わったら (⌘+Tab など) 拡大をやめる
    front_pid: Option<i32>,
    /// 始めた直後から前面に重なっていたウィンドウ (ツールパレットなど)。
    /// これ以外のウィンドウが新しく前面に重なったら (ダイアログなど) 拡大をやめる
    covering: Option<Vec<u32>>,
}

impl Session {
    /// 画面の点 `p` が対象のウィンドウの範囲内か
    pub fn contains(&self, p: CGPoint) -> bool {
        window_list::contains(&self.window.frame, p)
    }

    /// 点 `p` で拡大表示が見えていて、そこでの操作を本物のウィンドウに届けるべきか
    pub fn hit(&self, p: CGPoint) -> bool {
        self.showing && self.contains(p) && !self.above.iter().any(|r| window_list::contains(r, p))
    }

    /// 画面の点 → ウィンドウ内の座標
    fn local(&self, p: CGPoint) -> (f64, f64) {
        (
            p.x - self.window.frame.origin.x,
            p.y - self.window.frame.origin.y,
        )
    }

    /// 画面上で `p` に見えているものの、本当の位置 (クリックを届ける先)
    pub fn to_real(&self, p: CGPoint) -> CGPoint {
        let origin = self.window.frame.origin;
        let (x, y) = self.view.to_content(self.local(p));
        CGPoint::new(origin.x + x, origin.y + y)
    }

    /// クリックの直前に、前面にあるウィンドウ (開いたばかりのメニューなど) を調べ直す
    pub fn refresh_above(&mut self, overlay: Option<&Overlay>) {
        if let Some(overlay) = overlay {
            self.above = window_list::frames_above(overlay.window_number());
        }
    }
}

/// ウィンドウ `window` の拡大を始める (倍率はまだ等倍。続けて `zoom` を呼ぶ)
pub fn begin(state: &mut State, window: WindowInfo) {
    end(state);
    let id = state.next_session_id;
    state.next_session_id += 1;

    // 対象のウィンドウがいちばん前になければ前に出す。拡大表示の下で、手前にある別のウィンドウに
    // クリックが当たってしまわないように。相手のアプリの応答を待つことがあるので、
    // イベントタップのコールバックから戻った後で行う
    if window_list::frontmost_normal(state.own_pid) != Some(window.id) {
        let (pid, frame) = (window.pid, window.frame);
        DispatchQueue::main().exec_async(move || apps::raise_window(pid, frame));
    }

    let mtm = state.mtm;
    let overlay = state.overlay.get_or_insert_with(|| Overlay::new(mtm));
    overlay.set_frame(window.frame);

    capture::start(id, window.id);
    state.session = Some(Session {
        id,
        window,
        view: ViewZoom::new((window.frame.size.width, window.frame.size.height)),
        capture: None,
        showing: false,
        above: Vec::new(),
        dragging: false,
        timer: schedule_timer(),
        started: Instant::now(),
        front_pid: None,
        covering: None,
    });
}

/// 拡大をやめて等倍に戻す
pub fn end(state: &mut State) {
    let Some(session) = state.session.take() else {
        return;
    };
    session.timer.invalidate();
    if let Some(capture) = session.capture {
        capture.stop();
    }
    if let Some(overlay) = &mut state.overlay {
        overlay.hide();
    }
}

/// ピンチ量 (ホイール換算) で拡大・縮小する。`p` はピンチした場所。等倍まで戻ったら終わる
pub fn zoom(state: &mut State, units: f32, p: CGPoint) {
    let Some(session) = &mut state.session else {
        return;
    };
    let anchor = session.local(p);
    if !session.view.zoom(units, anchor, &state.zoom_settings) {
        return;
    }
    if !session.view.is_active() {
        end(state);
        return;
    }
    if let Some(overlay) = &state.overlay {
        overlay.update(&session.view);
    }
}

/// 2 本指スクロールで表示位置を動かす。動いたら true (false なら端に着いている)
pub fn pan(state: &mut State, delta: (f64, f64)) -> bool {
    let Some(session) = &mut state.session else {
        return false;
    };
    if !session.view.pan(delta) {
        return false;
    }
    if let Some(overlay) = &state.overlay {
        overlay.update(&session.view);
    }
    true
}

/// 撮影したフレームが届いた (メインスレッド)
pub fn on_frame(session_id: u64, surface: &AnyObject) {
    with_state(|state| {
        let (Some(session), Some(overlay)) = (&mut state.session, &mut state.overlay) else {
            return;
        };
        if session.id != session_id {
            return;
        }
        overlay.set_image(surface);
        if !session.showing {
            overlay.update(&session.view);
            overlay.show();
            session.showing = true;
            session.refresh_above(Some(overlay));
        }
    });
}

/// 撮影を始めた (メインスレッド)。そのセッションがもう終わっていたら止める
pub fn on_capture_ready(session_id: u64, capture: Capture) {
    let mut capture = Some(capture);
    with_state(|state| {
        if let Some(session) = &mut state.session
            && session.id == session_id
        {
            session.capture = capture.take();
        }
    });
    if let Some(unused) = capture {
        unused.stop();
    }
}

/// 撮影できなかった (メインスレッド)。`maybe_permission` なら画面収録の許可が無い可能性が高い
pub fn on_capture_failed(session_id: u64, message: String, maybe_permission: bool) {
    eprintln!("pinch-zoom: ウィンドウを撮影できませんでした: {message}");
    let alert = with_state(|state| {
        if state.session.as_ref().is_none_or(|s| s.id != session_id) {
            return None;
        }
        end(state);
        // ダイアログは 1 回だけ。with_state の外で出す (ダイアログはメッセージループを回すため)
        let first = !std::mem::replace(&mut state.capture_error_shown, true);
        (maybe_permission && first).then_some(state.mtm)
    })
    .flatten();
    if let Some(mtm) = alert {
        super::show_alert(
            mtm,
            "ウィンドウを拡大できませんでした",
            &format!(
                "画面収録の許可が必要です。\n\
                 システム設定 →「プライバシーとセキュリティ」→「画面収録とシステムオーディオ録音」で \
                 pinch-zoom をオンにしてから、メニューバーの pinch-zoom のメニューで「再起動」を選んでください。\n\n\
                 詳細: {message}"
            ),
        );
    }
}

fn schedule_timer() -> Retained<NSTimer> {
    let block = RcBlock::new(|_timer: NonNull<NSTimer>| tick());
    // SAFETY: ブロックはタイマーが保持する。タイマーはセッションの終わりに invalidate する
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(TICK_SECONDS, true, &block) }
}

/// 拡大中、0.1 秒ごとにウィンドウの状態を調べる
fn tick() {
    with_state(|state| {
        let own_pid = state.own_pid;
        let (Some(session), Some(overlay)) = (&mut state.session, &state.overlay) else {
            return;
        };

        // 閉じられた・最小化された・別のデスクトップに移った
        let Some(now) = window_list::find(session.window.id) else {
            end(state);
            return;
        };

        // 動いた・大きさが変わった
        if !same_rect(&now.frame, &session.window.frame) {
            let resized = now.frame.size.width != session.window.frame.size.width
                || now.frame.size.height != session.window.frame.size.height;
            session.window.frame = now.frame;
            session
                .view
                .resize((now.frame.size.width, now.frame.size.height));
            overlay.set_frame(now.frame);
            overlay.update(&session.view);
            if resized && let Some(capture) = &session.capture {
                capture.resize(now.frame);
            }
        }
        session.refresh_above(Some(overlay));

        if session.started.elapsed() < GRACE {
            return;
        }
        // 別のアプリに切り替わった (⌘+Tab、Dock のクリックなど)。
        // 対象のアプリが前面に来るのが遅れることもあるので、対象のアプリになったときは切り替えとみなさない
        let front = apps::frontmost_pid();
        if front == Some(session.window.pid) || session.front_pid.is_none() {
            session.front_pid = front;
        } else if session.front_pid != front {
            end(state);
            return;
        }
        // ダイアログやシートが開いた、別のウィンドウが前に来た → 拡大表示の下に隠れてしまうのでやめる
        let covering = window_list::covering_windows(&session.window, own_pid);
        match &session.covering {
            None => session.covering = Some(covering),
            Some(before) if covering.iter().any(|id| !before.contains(id)) => end(state),
            Some(_) => {}
        }
    });
}

fn same_rect(a: &CGRect, b: &CGRect) -> bool {
    a.origin.x == b.origin.x
        && a.origin.y == b.origin.y
        && a.size.width == b.size.width
        && a.size.height == b.size.height
}
