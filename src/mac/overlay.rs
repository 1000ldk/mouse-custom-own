//! 拡大表示用のウィンドウ。対象のウィンドウにぴったり重ね、撮影した映像を拡大して映す。
//!
//! - 枠も影も無いウィンドウを、普通のウィンドウより 1 つ上の層 (フローティング) に置く。
//!   Dock・メニューバー・メニューはさらに上の層なので、拡大中もそのまま使える
//! - マウスのイベントは受け取らず、下にある本物のウィンドウに素通しする (setIgnoresMouseEvents)。
//!   クリック位置の書き換えは tap.rs で行う
//! - 中身は Core Animation のレイヤー。撮影したフレーム (IOSurface) をそのままレイヤーの中身にするので、
//!   画像のコピーが要らず速い
//!
//! # 座標系
//! CGWindowList / CGEvent は「主ディスプレイの左上が原点、下向きが +y」、
//! NSWindow の位置 (Cocoa) は「主ディスプレイの左下が原点、上向きが +y」。
//! レイヤーも既定では左下が原点。

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGDisplayBounds, CGMainDisplayID};
use objc2_quartz_core::{CALayer, CATransaction};

use crate::view_zoom::ViewZoom;

pub struct Overlay {
    window: Retained<NSWindow>,
    root: Retained<CALayer>,
    content: Retained<CALayer>,
    visible: bool,
}

/// CGWindowList の座標 (左上原点) → Cocoa の座標 (左下原点)
fn to_cocoa(frame: CGRect) -> CGRect {
    let primary_height = CGDisplayBounds(CGMainDisplayID()).size.height;
    CGRect {
        origin: CGPoint {
            x: frame.origin.x,
            y: primary_height - (frame.origin.y + frame.size.height),
        },
        size: frame.size,
    }
}

/// Core Animation は値を変えると既定で 0.25 秒かけてアニメーションする。
/// 指に追従させたいので、アニメーションを切った状態で変更する
fn without_animation(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}

impl Overlay {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(100.0, 100.0));
        // SAFETY: 普通の初期化。閉じても解放されないようにしてから使う (所有権は Retained が持つ)
        let window = unsafe {
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            );
            window.setReleasedWhenClosed(false);
            window
        };
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        window.setLevel(NSFloatingWindowLevel);
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        // どのデスクトップ (Space) でも、フルスクリーンのアプリの上でも表示でき、
        // Mission Control や ⌘+` の切り替えの対象にならない
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );

        let root = CALayer::new();
        root.setMasksToBounds(true);
        let content = CALayer::new();
        root.addSublayer(&content);

        // レイヤーをこちらで用意して持たせる「レイヤーホスティング」のビュー
        // (setLayer を setWantsLayer より先に呼ぶ)
        let view = NSView::initWithFrame(mtm.alloc(), rect);
        view.setLayer(Some(&root));
        view.setWantsLayer(true);
        window.setContentView(Some(&view));

        Self {
            window,
            root,
            content,
            visible: false,
        }
    }

    /// ウィンドウ番号 (CGWindowID)。この番号より前面にあるウィンドウを調べるのに使う
    pub fn window_number(&self) -> u32 {
        self.window.windowNumber() as u32
    }

    /// 対象のウィンドウの位置と大きさ (CGWindowList の座標) に合わせる
    pub fn set_frame(&self, frame: CGRect) {
        self.window.setFrame_display(to_cocoa(frame), false);
        without_animation(|| {
            self.root
                .setFrame(CGRect::new(CGPoint::new(0.0, 0.0), frame.size));
        });
    }

    /// 撮影したフレーム (IOSurface) を中身にする
    pub fn set_image(&self, surface: &AnyObject) {
        without_animation(|| {
            // SAFETY: IOSurface はレイヤーの中身として使える型
            unsafe { self.content.setContents(Some(surface)) };
        });
    }

    /// 倍率と表示位置を反映する
    pub fn update(&self, view: &ViewZoom) {
        let (x, y_top, width, height) = view.content_frame();
        let window_height = view.size().1;
        // view_zoom は左上原点、レイヤーは左下原点なので上下を変換する
        let y = window_height - (y_top + height);
        without_animation(|| {
            self.content
                .setFrame(CGRect::new(CGPoint::new(x, y), CGSize::new(width, height)));
        });
    }

    pub fn show(&mut self) {
        if !self.visible {
            // 自分のアプリが前面になくても表示する
            self.window.orderFrontRegardless();
            self.visible = true;
        }
    }

    pub fn hide(&mut self) {
        self.window.orderOut(None);
        without_animation(|| unsafe { self.content.setContents(None) });
        self.visible = false;
    }
}
