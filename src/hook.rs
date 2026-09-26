//! 低レベルマウスフック (WH_MOUSE_LL)。
//!
//! # フックとは
//! Windows ではマウスやキーボードの入力は「システムの入力キュー」→「フォアグラウンドやカーソル下の
//! ウィンドウのスレッド」へと配送される。SetWindowsHookEx でフックを登録すると、
//! 配送の途中に自分の関数 (フックプロシージャ) を割り込ませ、イベントを覗いたり握りつぶしたりできる。
//!
//! 同じ種類のフックは複数のアプリが登録できて「フックチェーン」を作る。各フックは
//!   - CallNextHookEx を呼んで結果を返す → 次のフック / 本来の宛先へイベントが流れる
//!   - 0 以外を返して CallNextHookEx を呼ばない → イベントはそこで捨てられる (握りつぶし)
//!
//! のどちらかを選ぶ。
//!
//! # WH_MOUSE_LL (低レベル) の特徴
//! - DLL を他プロセスに注入しなくて良い (普通の WH_MOUSE は DLL 注入が必要)。
//! - コールバックは「フックを登録したスレッド」上で呼ばれる。OS は入力を処理する際に
//!   そのスレッドへ内部的なメッセージを送り、スレッドが GetMessage 等でメッセージを
//!   取りに来たタイミングでコールバックを実行する。
//!   → 登録したスレッドは必ずメッセージループを回し続けなければならない。
//! - コールバックはシステム全体のマウス入力を止めて待たせている。
//!   一定時間 (LowLevelHooksTimeout, 既定 約 300ms〜1s) 以内に返さないと無視され、
//!   Windows 7 以降は何度も遅れるとフックが黙って外される。
//!   → コールバック内では重い処理 (ファイル I/O、SendInput の連発、ダイアログ等) をしない。
//!   キー送信は PostMessage で自分のウィンドウに依頼し、コールバックから戻った後で行う。

use std::time::Instant;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, HHOOK, MSLLHOOKSTRUCT, PostMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

use crate::app::{AppState, with_state};
use crate::config::{RuleAction, find_rule};
use crate::gesture::ZoomDirection;
use crate::input::is_ctrl_down;
use crate::window::{WM_APP_SEND_KEYS, WM_APP_UPDATE_SCREEN_ZOOM};

/// 登録したフック。Drop で解除される (RAII)。
pub struct MouseHook(HHOOK);

impl MouseHook {
    pub fn install() -> windows::core::Result<Self> {
        // 第 3 引数はフックプロシージャを含むモジュール (自分の exe)。
        // 第 4 引数のスレッド ID = 0 は「全スレッド対象」。低レベルフックでは 0 しか使えない。
        unsafe {
            let module = GetModuleHandleW(None)?;
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(module.into()), 0)?;
            Ok(Self(hook))
        }
    }
}

impl Drop for MouseHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
        }
    }
}

/// フックでイベントをどう扱うか。
enum Verdict {
    /// 次のフックへ流す (何もしない)
    PassThrough,
    /// 握りつぶす
    Swallow,
}

/// フックプロシージャ。マウスが動くたびに呼ばれるので、対象外のイベントはすぐ返す。
///
/// - `code`  : HC_ACTION (0) なら処理対象。負の値なら何もせず CallNextHookEx に渡す決まり。
/// - `wparam`: メッセージの種類 (WM_MOUSEMOVE, WM_MOUSEWHEEL, ...)
/// - `lparam`: MSLLHOOKSTRUCT へのポインタ (座標、ホイール量、フラグ)
unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        match wparam.0 as u32 {
            WM_MOUSEWHEEL => {
                // SAFETY: WH_MOUSE_LL の HC_ACTION では lparam は必ず有効な MSLLHOOKSTRUCT を指す。
                let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                if let Verdict::Swallow = on_wheel(info) {
                    // 0 以外を返し、CallNextHookEx を呼ばない = このイベントは誰にも届かない。
                    return LRESULT(1);
                }
            }
            // 画面ズーム中はカーソルに合わせて表示位置を動かす (移動イベント自体は素通し)
            WM_MOUSEMOVE => on_move(),
            _ => {}
        }
    }
    // 自分が興味の無いイベントは必ず次へ回す。忘れると全アプリのマウスが効かなくなる。
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn on_wheel(info: &MSLLHOOKSTRUCT) -> Verdict {
    // ピンチ = Ctrl 押下中のホイール。Ctrl が押されていなければ普通のスクロールなので素通し。
    // (MSLLHOOKSTRUCT には修飾キーの情報が無いので GetAsyncKeyState で調べる)
    if !is_ctrl_down() {
        return Verdict::PassThrough;
    }

    // mouseData の上位 16bit が符号付きのホイール量 (上 = 正, 1 ノッチ = WHEEL_DELTA = 120)
    let delta = (info.mouseData >> 16) as u16 as i16 as i32;

    with_state(|app| {
        if !app.enabled {
            return Verdict::PassThrough;
        }
        // 画面ズーム中は、どのアプリの上でもピンチは画面ズームの操作にする
        // (拡大したまま VS Code に切り替えたら戻せない、ということが無いように)。
        if app.screen.is_active() {
            return screen_zoom(app, delta);
        }

        // フォアグラウンドのアプリに最初に一致したルールを探す。どれにも一致しなければ素通し。
        let Some(exe) = app.foreground.exe_name() else {
            return Verdict::PassThrough;
        };
        let Some(index) = find_rule(&app.rules, exe) else {
            return Verdict::PassThrough;
        };
        let rule = &app.rules[index];
        let (zoom_in, zoom_out) = match rule.action {
            RuleAction::Pass => return Verdict::PassThrough,
            RuleAction::ScreenZoom => return screen_zoom(app, delta),
            RuleAction::Zoom { zoom_in, zoom_out } => (zoom_in, zoom_out),
        };

        // 別のアプリ (ルール) に切り替わったら、前のアプリでの蓄積は持ち越さない
        if app.last_rule != Some(index) {
            app.pinch.reset();
            app.last_rule = Some(index);
        }

        if let Some(direction) = app.pinch.feed(delta, Instant::now(), &rule.settings) {
            let combo = match direction {
                ZoomDirection::In => zoom_in,
                ZoomDirection::Out => zoom_out,
            };
            // ここでは SendInput せず、自分のウィンドウにメッセージを「投函」するだけ。
            // PostMessage はキューに積んで即座に戻るので、フックを待たせない。
            // コールバックから戻った後、メッセージループがこれを取り出して window.rs で SendInput する。
            // 送るキーは整数に詰めて WPARAM で運ぶ。
            unsafe {
                let _ = PostMessageW(
                    Some(app.hwnd),
                    WM_APP_SEND_KEYS,
                    WPARAM(combo.to_bits()),
                    LPARAM(0),
                );
            }
        }
        // 対象アプリ上のピンチは、ズームしたかどうかに関わらず全部握りつぶす
        // (素通しすると、アプリ本来の Ctrl+ホイール動作 (VS Code ならエディタのフォントだけ拡大) も起きてしまう)。
        Verdict::Swallow
    })
    .unwrap_or(Verdict::PassThrough)
}

/// 画面ズーム: 閾値やクールダウンは使わず、delta をそのまま倍率に反映する (連続的に拡大するため)。
fn screen_zoom(app: &mut AppState, delta: i32) -> Verdict {
    if !app.magnifier_ready {
        return Verdict::PassThrough;
    }
    if app.screen.apply_delta(delta, &app.screen_settings) {
        request_screen_update(app);
    }
    Verdict::Swallow
}

fn on_move() {
    with_state(|app| {
        if app.screen.is_active() {
            request_screen_update(app);
        }
    });
}

/// 実際の MagSetFullscreenTransform は window.rs で (フックから戻った後に) 行う。
/// マウス移動は 1 秒に数百回来ることもあるので、未処理の依頼があるうちは追加で投函しない。
fn request_screen_update(app: &mut AppState) {
    if app.screen_update_pending {
        return;
    }
    app.screen_update_pending = true;
    unsafe {
        let _ = PostMessageW(
            Some(app.hwnd),
            WM_APP_UPDATE_SCREEN_ZOOM,
            WPARAM(0),
            LPARAM(0),
        );
    }
}
