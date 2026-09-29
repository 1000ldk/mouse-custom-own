//! イベントタップ (CGEventTap)。
//!
//! # イベントタップとは
//! マウスやキーボード、トラックパッドのジェスチャーのイベントがアプリに届く前に、
//! 途中に自分の関数を割り込ませる仕組み (Windows の低レベルフックに近い)。
//! コールバックでは、イベントを
//!   - そのまま返す → アプリに届く
//!   - 書き換えてから返す → 書き換えた内容でアプリに届く (ここではクリックの座標を書き換える)
//!   - null を返す → 握りつぶす (誰にも届かない)
//!
//! のどれかができる。書き換え・握りつぶしをするタップを作るにはアクセシビリティの許可が要る。
//!
//! # 気をつけること
//! - コールバックはメインスレッドの RunLoop で呼ばれる。時間がかかると macOS がタップを無効にする
//!   (kCGEventTapDisabledByTimeout が届くので、有効に戻す)
//! - マウスが動くたびに呼ばれるので、拡大していないときはすぐ返す

use std::ffi::c_void;
use std::ptr::{NonNull, null_mut};
use std::time::Instant;

use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventPhase, NSEventType};
use objc2_core_foundation::{CFMachPort, CFRunLoop, CGPoint, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};
use objc2_foundation::NSTimer;

use super::state::{State, with_state};
use super::window_list::{self, WindowInfo};
use super::{apps, input, menu, zoom};
use crate::config::{RuleAction, find_rule};
use crate::gesture::{ZoomDirection, units_from_scale};

/// トラックパッドのジェスチャーのイベントの種類 (NSEventType と同じ値)。
/// ピンチは macOS のバージョンによって「ジェスチャー」(29) として届くことも「拡大」(30) として
/// 届くこともあるので、どちらも受け取り、NSEvent に変換してから中身の種類で見分ける
const GESTURE: u32 = 29;
const MAGNIFY: u32 = 30;
/// 「スマートズーム」(2 本指のダブルタップ)
const SMART_MAGNIFY: u32 = 32;

/// ピンチの行き先。ピンチの始まりに、ポインタの下のウィンドウのアプリとルールから決める
#[derive(Debug, Clone, Copy)]
pub enum Target {
    /// 何もしない (アプリ本来のピンチ)
    Pass,
    /// rules[rule] のキーをアプリ `pid` に送る
    Keys { rule: usize, pid: i32 },
    /// ウィンドウを拡大する
    Window(WindowInfo),
}

enum Verdict {
    /// アプリに届ける (座標を書き換えた場合も含む)
    Pass,
    /// 握りつぶす
    Swallow,
}

/// タップを作る。アクセシビリティが許可されていなければ、許可されるまで 1 秒ごとに作り直す
pub fn install_when_permitted() {
    if try_install() {
        return;
    }
    menu::refresh();
    let block = RcBlock::new(|timer: NonNull<NSTimer>| {
        if try_install() {
            // SAFETY: タイマーはこのブロックの呼び出し中は有効
            unsafe { timer.as_ref() }.invalidate();
        }
    });
    // SAFETY: ブロックはタイマーが保持する
    let _ = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0, true, &block) };
}

fn try_install() -> bool {
    let bit = |kind: CGEventType| -> CGEventMask { 1 << kind.0 };
    let mask = (1 << GESTURE)
        | (1 << MAGNIFY)
        | (1 << SMART_MAGNIFY)
        | bit(CGEventType::ScrollWheel)
        | bit(CGEventType::MouseMoved)
        | bit(CGEventType::LeftMouseDown)
        | bit(CGEventType::LeftMouseUp)
        | bit(CGEventType::LeftMouseDragged)
        | bit(CGEventType::RightMouseDown)
        | bit(CGEventType::RightMouseUp)
        | bit(CGEventType::RightMouseDragged)
        | bit(CGEventType::OtherMouseDown)
        | bit(CGEventType::OtherMouseUp)
        | bit(CGEventType::OtherMouseDragged);
    // SAFETY: コールバックは下の関数。user_info は使わない
    let port = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            mask,
            Some(callback),
            null_mut(),
        )
    };
    let Some(port) = port else {
        return false; // アクセシビリティが許可されていない
    };
    // タップは Mach ポートとして届くので、RunLoop のソースにしてメインスレッドの RunLoop に登録する。
    // CommonModes に入れておくと、メニューやダイアログの表示中も動く
    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
        return false;
    };
    let Some(run_loop) = CFRunLoop::main() else {
        return false;
    };
    // SAFETY: kCFRunLoopCommonModes は CoreFoundation の定数
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&port, true);
    with_state(|state| state.tap = Some(port));
    menu::refresh();
    true
}

unsafe extern "C-unwind" fn callback(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    _user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: event はコールバックの間は有効
    let cg_event = unsafe { event.as_ref() };
    let verdict = match kind {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            // 時間がかかりすぎた等で無効にされた → 有効に戻す
            with_state(|state| {
                if let Some(tap) = &state.tap {
                    CGEvent::tap_enable(tap, true);
                }
            });
            Verdict::Pass
        }
        CGEventType::ScrollWheel => on_scroll(cg_event),
        k if matches!(k.0, GESTURE | MAGNIFY | SMART_MAGNIFY) => on_gesture(cg_event),
        _ => on_mouse(kind, cg_event),
    };
    match verdict {
        Verdict::Pass => event.as_ptr(),
        Verdict::Swallow => null_mut(),
    }
}

/// トラックパッドのジェスチャー。ピンチとスマートズーム以外 (2 本指スクロールの途中経過など) はそのまま通す
fn on_gesture(event: &CGEvent) -> Verdict {
    // 種類・拡大率・始まり / 終わりは NSEvent に変換すると読める
    let Some(ns_event) = NSEvent::eventWithCGEvent(event) else {
        return Verdict::Pass;
    };
    let p = CGEvent::location(Some(event));
    match ns_event.r#type() {
        NSEventType::Magnify => {
            let scale = 1.0 + ns_event.magnification();
            let phase = ns_event.phase();
            with_state(|state| handle_magnify(state, scale, phase, p)).unwrap_or(Verdict::Pass)
        }
        NSEventType::SmartMagnify => {
            with_state(|state| handle_smart_magnify(state, p)).unwrap_or(Verdict::Pass)
        }
        _ => Verdict::Pass,
    }
}

fn handle_magnify(state: &mut State, scale: f64, phase: NSEventPhase, p: CGPoint) -> Verdict {
    if !state.enabled {
        state.gesture = None;
        return Verdict::Pass;
    }
    // 行き先はピンチの始まりで決め、終わるまで変えない
    if phase.contains(NSEventPhase::Began) || state.gesture.is_none() {
        state.gesture = Some(pick_target(state, p));
    }
    let target = state.gesture.unwrap_or(Target::Pass);
    if phase.intersects(NSEventPhase::Ended | NSEventPhase::Cancelled) {
        state.gesture = None;
    }

    let units = units_from_scale(scale);
    match target {
        Target::Pass => Verdict::Pass,
        Target::Keys { rule, pid } => {
            let Some(rule_ref) = state.rules.get(rule) else {
                return Verdict::Pass;
            };
            let RuleAction::Zoom { zoom_in, zoom_out } = rule_ref.action else {
                return Verdict::Pass;
            };
            let settings = rule_ref.settings;
            // 別のルール (アプリ) に切り替わったら、前の蓄積は持ち越さない
            if state.last_rule != Some(rule) {
                state.pinch.reset();
                state.last_rule = Some(rule);
            }
            if let Some(direction) = state.pinch.feed(units, Instant::now(), &settings) {
                let combo = match direction {
                    ZoomDirection::In => zoom_in,
                    ZoomDirection::Out => zoom_out,
                };
                input::send_combo(combo, pid);
            }
            Verdict::Swallow
        }
        Target::Window(window) => {
            if state.session.is_none() {
                if units <= 0.0 {
                    // 等倍のまま縮小しようとしている: 何も起きないが、アプリ本来のズームにもさせない
                    return Verdict::Swallow;
                }
                zoom::begin(state, window);
            }
            zoom::zoom(state, units, p);
            Verdict::Swallow
        }
    }
}

/// ポインタの下のウィンドウとルールから、ピンチの行き先を決める
fn pick_target(state: &mut State, p: CGPoint) -> Target {
    if let Some(session) = &state.session {
        // 拡大中のウィンドウの上なら、そのまま続ける
        if session.contains(p) {
            return Target::Window(session.window);
        }
        // 別の場所でピンチした → いまの拡大はやめて、そこを対象にする
        zoom::end(state);
    }
    let Some(window) = window_list::window_at(p, state.own_pid) else {
        return Target::Pass; // Dock やメニューバーなど
    };
    let names = apps::names_of(window.pid);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let Some(rule) = find_rule(&state.rules, &names) else {
        return Target::Pass;
    };
    match state.rules[rule].action {
        RuleAction::Pass => Target::Pass,
        RuleAction::Zoom { .. } => Target::Keys {
            rule,
            pid: window.pid,
        },
        // Mac には画面全体のズームが無いので、screen_zoom もウィンドウのズームとして扱う
        RuleAction::WindowZoom | RuleAction::ScreenZoom => Target::Window(window),
    }
}

/// 2 本指のダブルタップ (スマートズーム): 拡大中なら等倍に戻す、そうでなければその場所を 2 倍に拡大
fn handle_smart_magnify(state: &mut State, p: CGPoint) -> Verdict {
    if !state.enabled {
        return Verdict::Pass;
    }
    if state.session.as_ref().is_some_and(|s| s.contains(p)) {
        zoom::end(state);
        return Verdict::Swallow;
    }
    let Target::Window(window) = pick_target(state, p) else {
        return Verdict::Pass;
    };
    zoom::begin(state, window);
    // ちょうど 2 倍 (倍率を 2 倍にするピンチ量。向きの反転設定の影響を受けないように打ち消す)
    let settings = state.zoom_settings;
    let doubling = if settings.invert {
        -settings.delta_per_doubling
    } else {
        settings.delta_per_doubling
    };
    zoom::zoom(state, doubling, p);
    Verdict::Swallow
}

/// スクロール: 拡大中は、拡大表示の中で見る場所を動かす。端に着いていたらアプリに渡す
fn on_scroll(event: &CGEvent) -> Verdict {
    let p = CGEvent::location(Some(event));
    with_state(|state| {
        if !state.session.as_ref().is_some_and(|s| s.hit(p)) {
            return Verdict::Pass;
        }
        // 1 回のイベントで動く量 (ポイント)。正 = 内容が下 / 右に動く向き
        // (「ナチュラルなスクロール」の設定は反映済み)
        let delta = |field| CGEvent::integer_value_field(Some(event), field) as f64;
        let dy = delta(CGEventField::ScrollWheelEventPointDeltaAxis1);
        let dx = delta(CGEventField::ScrollWheelEventPointDeltaAxis2);
        if zoom::pan(state, (dx, dy)) {
            return Verdict::Swallow;
        }
        // 端に着いている → 見えている場所にあるもの (の本当の位置) をスクロールさせる
        if let Some(session) = &state.session {
            CGEvent::set_location(Some(event), session.to_real(p));
        }
        Verdict::Pass
    })
    .unwrap_or(Verdict::Pass)
}

/// クリック・ドラッグ・マウス移動: 拡大中は「見えている位置」→「本当の位置」に書き換える
fn on_mouse(kind: CGEventType, event: &CGEvent) -> Verdict {
    let p = CGEvent::location(Some(event));
    with_state(|state| {
        let Some(session) = &mut state.session else {
            return;
        };
        let mut end = false;
        let remap = match kind {
            CGEventType::LeftMouseDown
            | CGEventType::RightMouseDown
            | CGEventType::OtherMouseDown => {
                // 開いたばかりのメニューの上かもしれないので、前面のウィンドウを調べ直す
                session.refresh_above(state.overlay.as_ref());
                if session.hit(p) {
                    session.dragging = true;
                    true
                } else {
                    // ウィンドウの外をクリックした → 拡大をやめる
                    // (Dock やメニューなど、ウィンドウの範囲内で前面にあるもののクリックでは続ける)
                    end = !session.contains(p);
                    false
                }
            }
            CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged
            | CGEventType::OtherMouseDragged => session.dragging || session.hit(p),
            CGEventType::LeftMouseUp | CGEventType::RightMouseUp | CGEventType::OtherMouseUp => {
                let remap = session.dragging || session.hit(p);
                session.dragging = false;
                remap
            }
            CGEventType::MouseMoved => session.hit(p),
            _ => false,
        };
        if remap {
            CGEvent::set_location(Some(event), session.to_real(p));
        }
        if end {
            zoom::end(state);
        }
    });
    Verdict::Pass
}
