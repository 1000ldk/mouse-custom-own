//! Windows 向けか Mac 向けか。
//!
//! キーコードの体系や設定ファイルの既定値は OS ごとに違う。`#[cfg]` で切り替えるだけだと
//! 片方の OS のテストしか実行できないので、値として持っておき、テストでは両方を試す。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Mac,
}

impl Platform {
    /// いまビルドしている OS
    pub const NATIVE: Platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Windows
    };
}
