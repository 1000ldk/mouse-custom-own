//! macOS の権限 (プライバシーとセキュリティ) の確認と要求。
//!
//! - アクセシビリティ: イベントを書き換える・握りつぶすイベントタップ、キーの送信、ウィンドウを前面に出す
//! - 画面収録: ScreenCaptureKit でウィンドウを撮る
//!
//! どちらも初回は macOS が許可を求めるダイアログを出し、ユーザーがシステム設定でオンにする。
//! 権限は「アプリ (の署名)」に対して与えられる。

use std::process::Command;

use objc2_application_services::{AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};

/// アクセシビリティが許可されているか。許可されていなければ、macOS の許可ダイアログを出す
pub fn request_accessibility() -> bool {
    // SAFETY: kAXTrustedCheckOptionPrompt は ApplicationServices の定数
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: 「CFString → CFBoolean」の辞書を渡している
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) }
}

/// 画面収録が許可されているか (ダイアログは出さない)
pub fn screen_capture_allowed() -> bool {
    CGPreflightScreenCaptureAccess()
}

/// 画面収録の許可を求める (macOS のダイアログを出し、システム設定の一覧に追加される)
pub fn request_screen_capture() {
    CGRequestScreenCaptureAccess();
}

/// システム設定の「プライバシーとセキュリティ」の該当ページを開く
/// (`anchor`: "Privacy_Accessibility" / "Privacy_ScreenCapture")
pub fn open_settings(anchor: &str) {
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    let _ = Command::new("open").arg(url).spawn();
}
