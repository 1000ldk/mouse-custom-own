//! 設定ファイル (config.toml) の読み書き。
//!
//! 設定ファイルは exe と同じフォルダの `config.toml`。無ければ既定値で自動生成する。
//! Win32 に依存しないので Linux でもテストできる。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::gesture::PinchSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 蓄積 delta がこの値に達したら 1 段ズームする。
    /// 物理マウスのホイール 1 ノッチ = 120。小さいほど敏感。
    pub threshold: i32,
    /// 1 段ズームした後、ピンチ入力を無視するミリ秒。
    pub cooldown_ms: u64,
    /// イベントがこのミリ秒以上途切れたら「別のピンチ」とみなして蓄積をリセットする。
    pub gesture_gap_ms: u64,
    /// true ならズームの向きを反転する。
    pub invert: bool,
    /// 反応させる実行ファイル名 (大文字小文字は区別しない)。
    pub target_processes: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            threshold: 120,
            cooldown_ms: 400,
            gesture_gap_ms: 500,
            invert: false,
            target_processes: vec!["Code.exe".to_string()],
        }
    }
}

const TEMPLATE_HEADER: &str = "\
# pinch-zoom の設定ファイル
# 編集後はトレイアイコンの右クリックメニュー「設定を再読み込み」で反映されます。
#
# threshold       : 蓄積したホイール量がこの値に達したら 1 段ズーム (120 = マウスホイール 1 ノッチ)
# cooldown_ms     : 1 段ズームした後、入力を無視する時間 (ミリ秒)
# gesture_gap_ms  : 入力がこの時間途切れたら別のピンチとみなす (ミリ秒)
# invert          : true でズーム方向を反転
# target_processes: 反応させるアプリの実行ファイル名 (例: \"Cursor.exe\", \"Code - Insiders.exe\")

";

impl Config {
    pub fn pinch_settings(&self) -> PinchSettings {
        PinchSettings {
            threshold: self.threshold.max(1),
            cooldown: Duration::from_millis(self.cooldown_ms),
            gesture_gap: Duration::from_millis(self.gesture_gap_ms),
            invert: self.invert,
        }
    }

    /// `exe_name` (パスではなくファイル名) が対象アプリかどうか。
    pub fn is_target(&self, exe_name: &str) -> bool {
        self.target_processes
            .iter()
            .any(|t| t.eq_ignore_ascii_case(exe_name))
    }

    /// 設定ファイルのパス: exe と同じフォルダの config.toml
    pub fn default_path() -> PathBuf {
        std::env::current_exe()
            .map(|p| p.with_file_name("config.toml"))
            .unwrap_or_else(|_| PathBuf::from("config.toml"))
    }

    /// 読み込む。ファイルが無ければ既定値で作成してから返す。
    pub fn load_or_create(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}:\n{e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                // 書き込めなくても (読み取り専用フォルダなど) 既定値で動かす
                let _ = std::fs::write(path, config.to_toml());
                Ok(config)
            }
            Err(e) => Err(format!("{}:\n{e}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        let body = toml::to_string_pretty(self).expect("Config is always serializable");
        format!("{TEMPLATE_HEADER}{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let text = Config::default().to_toml();
        let parsed = Config::parse(&text).unwrap();
        assert_eq!(parsed.threshold, 120);
        assert_eq!(parsed.target_processes, vec!["Code.exe"]);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let parsed = Config::parse("target_processes = [\"Cursor.exe\"]").unwrap();
        assert_eq!(parsed.cooldown_ms, 400);
        assert!(parsed.is_target("cursor.EXE"));
        assert!(!parsed.is_target("Code.exe"));
    }
}
