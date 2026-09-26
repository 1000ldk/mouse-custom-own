//! Magnification API で画面全体を拡大する。
//!
//! Windows 標準の「拡大鏡」アプリも内部で使っている API。
//! `MagSetFullscreenTransform(倍率, 左上X, 左上Y)` を呼ぶと、画面全体が
//! 「(左上X, 左上Y) から始まる領域を 倍率 倍に拡大した表示」に切り替わる。
//! 倍率 1.0 で元に戻る。拡大鏡アプリのような UI ウィンドウは出ない。
//!
//! 注意:
//! - 拡大されるのは「見た目」だけで、マウスの座標は変換されない。
//!   そのためオフセットはカーソル位置から計算し、カーソルの下が常に正しい位置になるようにしている
//!   (screen_zoom.rs 参照)。カーソルが動くたびにオフセットを更新する。
//! - 全画面拡大は同時に 1 つのアプリしか使えない。Windows の拡大鏡が起動していると失敗する。

use windows::Win32::UI::Magnification::{
    MagInitialize, MagSetFullscreenTransform, MagUninitialize,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// MagInitialize 済みであることを表すハンドル。Drop で倍率を戻して後片付けする。
pub struct Magnifier(());

impl Magnifier {
    pub fn init() -> Option<Self> {
        unsafe { MagInitialize().as_bool().then_some(Self(())) }
    }
}

impl Drop for Magnifier {
    fn drop(&mut self) {
        // 拡大したまま終了すると画面が戻らなくなることがあるので、必ず 1 倍に戻す
        set_transform(1.0, (0, 0));
        unsafe {
            let _ = MagUninitialize();
        }
    }
}

/// 主モニターの大きさ (ピクセル)
pub fn primary_screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// 倍率と表示オフセットを設定する。失敗したら false。
pub fn set_transform(level: f32, offset: (i32, i32)) -> bool {
    unsafe { MagSetFullscreenTransform(level, offset.0, offset.1).as_bool() }
}
