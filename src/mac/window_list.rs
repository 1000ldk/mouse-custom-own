//! 画面上のウィンドウの一覧 (CGWindowListCopyWindowInfo)。
//!
//! 全アプリのウィンドウの番号・持ち主のプロセス・位置と大きさ・重なり順を調べられる。
//! 結果は前面 → 背面の順。座標は「主ディスプレイの左上が (0, 0)、下向きが +y」(単位はポイント) で、
//! CGEvent のマウス座標と同じ座標系。
//!
//! 1 回の呼び出しに数ミリ秒かかることがあるので、マウスが動くたびには呼ばない。

use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect,
};
use objc2_core_graphics::{
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGWindowAlpha, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer, kCGWindowNumber,
    kCGWindowOwnerPID,
};

/// 普通のアプリのウィンドウの重なりの層 (kCGNormalWindowLevel)。
/// Dock やメニューバー、通知などは別の層にいる
const NORMAL_LAYER: i64 = 0;

#[derive(Debug, Clone, Copy)]
pub struct WindowInfo {
    pub id: u32,
    pub pid: i32,
    pub frame: CGRect,
    layer: i64,
    alpha: f64,
    onscreen: bool,
}

impl WindowInfo {
    fn is_visible_normal(&self) -> bool {
        self.layer == NORMAL_LAYER && self.alpha > 0.0
    }
}

pub fn contains(rect: &CGRect, p: CGPoint) -> bool {
    p.x >= rect.origin.x
        && p.y >= rect.origin.y
        && p.x < rect.origin.x + rect.size.width
        && p.y < rect.origin.y + rect.size.height
}

fn intersects(a: &CGRect, b: &CGRect) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
}

fn list(option: CGWindowListOption, relative_to: u32) -> Vec<WindowInfo> {
    let Some(array) = CGWindowListCopyWindowInfo(option, relative_to) else {
        return Vec::new();
    };
    // SAFETY: CGWindowListCopyWindowInfo は「CFString → 値」の辞書の配列を返す
    let array: CFRetained<CFArray<CFDictionary<CFString, CFType>>> =
        unsafe { CFRetained::cast_unchecked(array) };
    array.iter().filter_map(|d| parse(&d)).collect()
}

fn parse(d: &CFDictionary<CFString, CFType>) -> Option<WindowInfo> {
    // SAFETY: kCGWindow* は CoreGraphics が定義している定数の文字列
    let (number_key, pid_key, layer_key, alpha_key, bounds_key, onscreen_key) = unsafe {
        (
            kCGWindowNumber,
            kCGWindowOwnerPID,
            kCGWindowLayer,
            kCGWindowAlpha,
            kCGWindowBounds,
            kCGWindowIsOnscreen,
        )
    };
    let number = |key: &CFString| d.get(key)?.downcast::<CFNumber>().ok();

    let bounds = d.get(bounds_key)?.downcast::<CFDictionary>().ok()?;
    let mut frame = CGRect::default();
    // SAFETY: bounds は CGRect を表す辞書、frame は書き込み可能な CGRect
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&bounds), &mut frame) } {
        return None;
    }
    Some(WindowInfo {
        id: number(number_key)?.as_i64()? as u32,
        pid: number(pid_key)?.as_i64()? as i32,
        frame,
        layer: number(layer_key).and_then(|n| n.as_i64()).unwrap_or(0),
        alpha: number(alpha_key).and_then(|n| n.as_f64()).unwrap_or(1.0),
        // 画面に出ているウィンドウにだけ付いている項目
        onscreen: d
            .get(onscreen_key)
            .and_then(|v| v.downcast::<CFBoolean>().ok())
            .is_some_and(|b| b.as_bool()),
    })
}

/// 画面上に出ているウィンドウ (前面 → 背面)。デスクトップのアイコンなどは除く
fn onscreen() -> Vec<WindowInfo> {
    list(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    )
}

/// 点 `p` にあるいちばん前面のウィンドウ。自分 (`own_pid`) のウィンドウは無視する。
/// それが普通のアプリのウィンドウでなければ (Dock、メニューバー、通知など) None。
pub fn window_at(p: CGPoint, own_pid: i32) -> Option<WindowInfo> {
    let front = onscreen()
        .into_iter()
        .find(|w| w.pid != own_pid && w.alpha > 0.0 && contains(&w.frame, p))?;
    front.is_visible_normal().then_some(front)
}

/// ウィンドウ番号から、いまの情報を調べる。閉じられた / 画面に出ていなければ None
pub fn find(id: u32) -> Option<WindowInfo> {
    list(CGWindowListOption::OptionIncludingWindow, id)
        .into_iter()
        .find(|w| w.id == id && w.onscreen)
}

/// 普通のアプリのウィンドウのうち、いちばん前面にあるものの番号 (自分のウィンドウは除く)
pub fn frontmost_normal(own_pid: i32) -> Option<u32> {
    onscreen()
        .into_iter()
        .find(|w| w.pid != own_pid && w.is_visible_normal())
        .map(|w| w.id)
}

/// ウィンドウ `id` より前面にあるウィンドウの範囲 (Dock、メニューバー、メニューなど)
pub fn frames_above(id: u32) -> Vec<CGRect> {
    list(CGWindowListOption::OptionOnScreenAboveWindow, id)
        .into_iter()
        .filter(|w| w.alpha > 0.0)
        .map(|w| w.frame)
        .collect()
}

/// ウィンドウ `target` より前面にあって重なっている、普通のアプリのウィンドウ (自分以外) の番号。
/// ダイアログやシートが開いた、別のアプリのウィンドウが前に来た、などを検出するのに使う
pub fn covering_windows(target: &WindowInfo, own_pid: i32) -> Vec<u32> {
    list(CGWindowListOption::OptionOnScreenAboveWindow, target.id)
        .into_iter()
        .filter(|w| {
            w.pid != own_pid && w.is_visible_normal() && intersects(&w.frame, &target.frame)
        })
        .map(|w| w.id)
        .collect()
}
