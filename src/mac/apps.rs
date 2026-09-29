//! アプリ (プロセス) の名前を調べる、前面に出す。

use std::ptr::NonNull;

use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};

/// ルールの `apps` と照らし合わせる、アプリの呼び名の候補:
/// バンドル ID ("com.apple.TextEdit")、アプリ名 ("テキストエディット")、実行ファイル名 ("TextEdit")
pub fn names_of(pid: i32) -> Vec<String> {
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    if let Some(id) = app.bundleIdentifier() {
        names.push(id.to_string());
    }
    if let Some(name) = app.localizedName() {
        names.push(name.to_string());
    }
    if let Some(file) = app.executableURL().and_then(|url| url.lastPathComponent()) {
        names.push(file.to_string());
    }
    names
}

/// いま前面にある (キーボード入力を受け取る) アプリのプロセス ID
pub fn frontmost_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

/// アプリ `pid` を前面に出し、その中で位置と大きさが `frame` のウィンドウをいちばん前に出す。
///
/// アクセシビリティ API (AXUIElement) で、ユーザーが操作するのと同じように
/// 「アプリを前面にする」「ウィンドウを前面にする」を依頼する。
/// 相手のアプリが応答しないと待たされるので、待ち時間の上限を短くしておく。
pub fn raise_window(pid: i32, frame: CGRect) {
    // SAFETY: 各 AX 関数には有効な AXUIElement と CFString を渡している
    unsafe {
        let app = AXUIElement::new_application(pid);
        app.set_messaging_timeout(0.3);
        if let Some(windows) =
            attribute(&app, "AXWindows").and_then(|v| v.downcast::<CFArray>().ok())
        {
            // SAFETY: AXWindows 属性は AXUIElement の配列
            let windows: CFRetained<CFArray<AXUIElement>> = CFRetained::cast_unchecked(windows);
            if let Some(window) = windows
                .iter()
                .find(|w| window_frame(w).is_some_and(|f| same_frame(&f, &frame)))
            {
                window.perform_action(&CFString::from_static_str("AXRaise"));
            }
        }
        app.set_attribute_value(
            &CFString::from_static_str("AXFrontmost"),
            CFBoolean::new(true),
        );
    }
}

/// AX の属性を 1 つ読む
unsafe fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = std::ptr::null();
    let err = unsafe {
        element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value))
    };
    if err != AXError::Success {
        return None;
    }
    // SAFETY: Copy 関数で受け取った値は、こちらが解放する責任を持つ (+1 の参照)
    NonNull::new(value.cast_mut()).map(|v| unsafe { CFRetained::from_raw(v) })
}

/// AX で見たウィンドウの位置と大きさ
unsafe fn window_frame(window: &AXUIElement) -> Option<CGRect> {
    let position = unsafe { attribute(window, "AXPosition") }?
        .downcast::<AXValue>()
        .ok()?;
    let size = unsafe { attribute(window, "AXSize") }?
        .downcast::<AXValue>()
        .ok()?;
    let mut origin = CGPoint::default();
    let mut extent = CGSize::default();
    // SAFETY: 型に合った書き込み先を渡している
    let ok = unsafe {
        position.value(AXValueType::CGPoint, NonNull::from(&mut origin).cast())
            && size.value(AXValueType::CGSize, NonNull::from(&mut extent).cast())
    };
    ok.then_some(CGRect {
        origin,
        size: extent,
    })
}

fn same_frame(a: &CGRect, b: &CGRect) -> bool {
    let near = |x: f64, y: f64| (x - y).abs() <= 2.0;
    near(a.origin.x, b.origin.x)
        && near(a.origin.y, b.origin.y)
        && near(a.size.width, b.size.width)
        && near(a.size.height, b.size.height)
}
