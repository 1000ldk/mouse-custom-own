//! 設定ファイル (config.toml) の読み書き。
//!
//! 設定ファイルは exe と同じフォルダの `config.toml`。無ければ既定値で自動生成する。
//! Win32 に依存しないので Linux でもテストできる。
//!
//! # ルール
//! `[[rules]]` を上から順に調べ、フォアグラウンドのアプリに最初に一致したルールを使う。
//! - `apps`     : 実行ファイル名のリスト。`"*"` は全アプリに一致
//! - `zoom_in` / `zoom_out` : ピンチアウト / ピンチインで送るキー ("Ctrl+NumpadAdd" など)
//! - `screen_zoom = true` : 画面全体をピンチ量に合わせて滑らかに拡大する (screen_zoom.rs)
//! - `pass = true` : 何もせずアプリに Ctrl+ホイールをそのまま渡す (ブラウザ等、自前でズームできるアプリ用)
//! - `threshold` / `cooldown_ms` : そのルールだけ全体設定を上書きしたいとき

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::gesture::PinchSettings;
use crate::keys::KeyCombo;
use crate::screen_zoom::ScreenZoomSettings;

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
    /// 画面ズームの最大倍率。
    pub screen_zoom_max: f32,
    /// 画面ズームで倍率を 2 倍にするのに必要なホイール量。小さいほど速く拡大する。
    pub screen_zoom_speed: f32,
    /// アプリごとのルール (上から順に評価)。
    pub rules: Vec<RuleConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleConfig {
    pub apps: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pass: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub screen_zoom: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom_in: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown_ms: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            threshold: 120,
            cooldown_ms: 400,
            gesture_gap_ms: 500,
            invert: false,
            screen_zoom_max: 8.0,
            screen_zoom_speed: 400.0,
            rules: vec![
                // VS Code 系: ウィンドウ全体のズーム
                RuleConfig {
                    apps: vec!["Code.exe".into(), "Cursor.exe".into()],
                    zoom_in: Some("Ctrl+NumpadAdd".into()),
                    zoom_out: Some("Ctrl+NumpadSubtract".into()),
                    ..Default::default()
                },
                // それ以外の全アプリ: 画面全体をピンチ量に合わせて滑らかに拡大
                RuleConfig {
                    apps: vec!["*".into()],
                    screen_zoom: true,
                    ..Default::default()
                },
            ],
        }
    }
}

/// 解析済みのルール。フック内で文字列処理をしないよう、読み込み時に変換しておく。
#[derive(Debug, Clone)]
pub struct Rule {
    apps: Vec<String>,
    pub action: RuleAction,
    pub settings: PinchSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    /// イベントを素通しする
    Pass,
    /// 画面全体を連続的に拡大する
    ScreenZoom,
    /// イベントを握りつぶし、蓄積に応じてキーを送る
    Zoom {
        zoom_in: KeyCombo,
        zoom_out: KeyCombo,
    },
}

impl Rule {
    pub fn matches(&self, exe_name: &str) -> bool {
        self.apps
            .iter()
            .any(|a| a == "*" || a.eq_ignore_ascii_case(exe_name))
    }
}

const TEMPLATE_HEADER: &str = "\
# pinch-zoom の設定ファイル
# 編集後はトレイアイコンの右クリックメニュー「設定を再読み込み」で反映されます。
#
# threshold      : 蓄積したホイール量がこの値に達したら 1 段ズーム (120 = マウスホイール 1 ノッチ)
# cooldown_ms    : 1 段ズームした後、入力を無視する時間 (ミリ秒)
# gesture_gap_ms : 入力がこの時間途切れたら別のピンチとみなす (ミリ秒)
# invert         : true でズーム方向を反転
# screen_zoom_max   : 画面ズームの最大倍率
# screen_zoom_speed : 画面ズームで倍率を 2 倍にするのに必要なホイール量 (小さいほど速い)
#
# [[rules]] は上から順に調べ、最初に一致したものが使われます。
#   apps      : 実行ファイル名のリスト。\"*\" は全アプリ
#   zoom_in   : ピンチアウトで送るキー (例: \"Ctrl+NumpadAdd\", \"Ctrl+Plus\", \"Win+NumpadAdd\")
#   zoom_out  : ピンチインで送るキー
#   screen_zoom : true なら、キーを送る代わりに画面全体をピンチ量に合わせて滑らかに拡大
#                 (ピンチした場所が中心。ピンチインで 1 倍に戻すと終了)
#   pass      : true なら何もせず、アプリ本来のピンチ動作に任せる
#   threshold / cooldown_ms : そのルールだけ上書き
#
# 例: ブラウザは自前のピンチズームを使う (\"*\" のルールより上に書く)
#   [[rules]]
#   apps = [\"chrome.exe\", \"msedge.exe\", \"firefox.exe\"]
#   pass = true

";

impl Config {
    fn base_settings(&self) -> PinchSettings {
        PinchSettings {
            threshold: self.threshold.max(1),
            cooldown: Duration::from_millis(self.cooldown_ms),
            gesture_gap: Duration::from_millis(self.gesture_gap_ms),
            invert: self.invert,
        }
    }

    /// ルールを解析する。キー名の誤りなどはここでエラーにする。
    pub fn compile_rules(&self) -> Result<Vec<Rule>, String> {
        let base = self.base_settings();
        self.rules
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let at = |e: String| format!("rules[{}] ({:?}): {e}", i + 1, r.apps);
                if [
                    r.pass,
                    r.screen_zoom,
                    r.zoom_in.is_some() || r.zoom_out.is_some(),
                ]
                .iter()
                .filter(|&&b| b)
                .count()
                    > 1
                {
                    return Err(at(
                        "pass / screen_zoom / zoom_in・zoom_out はどれか 1 つだけ指定してください"
                            .into(),
                    ));
                }
                let action = if r.pass {
                    RuleAction::Pass
                } else if r.screen_zoom {
                    RuleAction::ScreenZoom
                } else {
                    let parse = |k: &Option<String>, name: &str| {
                        let text = k
                            .as_deref()
                            .ok_or_else(|| at(format!("{name} がありません")))?;
                        KeyCombo::parse(text).map_err(at)
                    };
                    RuleAction::Zoom {
                        zoom_in: parse(&r.zoom_in, "zoom_in")?,
                        zoom_out: parse(&r.zoom_out, "zoom_out")?,
                    }
                };
                let mut settings = base;
                if let Some(t) = r.threshold {
                    settings.threshold = t.max(1);
                }
                if let Some(c) = r.cooldown_ms {
                    settings.cooldown = Duration::from_millis(c);
                }
                Ok(Rule {
                    apps: r.apps.clone(),
                    action,
                    settings,
                })
            })
            .collect()
    }

    pub fn screen_zoom_settings(&self) -> ScreenZoomSettings {
        ScreenZoomSettings {
            max_level: self.screen_zoom_max,
            delta_per_doubling: self.screen_zoom_speed,
            invert: self.invert,
        }
    }

    /// 設定ファイルのパス: exe と同じフォルダの config.toml
    pub fn default_path() -> PathBuf {
        std::env::current_exe()
            .map(|p| p.with_file_name("config.toml"))
            .unwrap_or_else(|_| PathBuf::from("config.toml"))
    }

    /// 読み込んでルールまで解析する。ファイルが無ければ既定値で作成してから返す。
    pub fn load_or_create(path: &Path) -> Result<(Config, Vec<Rule>), String> {
        let config = match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}:\n{e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                // 書き込めなくても (読み取り専用フォルダなど) 既定値で動かす
                let _ = std::fs::write(path, config.to_toml());
                config
            }
            Err(e) => return Err(format!("{}:\n{e}", path.display())),
        };
        let rules = config
            .compile_rules()
            .map_err(|e| format!("{}:\n{e}", path.display()))?;
        Ok((config, rules))
    }

    pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        let body = toml::to_string_pretty(self).expect("Config is always serializable");
        format!("{TEMPLATE_HEADER}{body}")
    }
}

/// 実行ファイル名に最初に一致したルールの番号。
pub fn find_rule(rules: &[Rule], exe_name: &str) -> Option<usize> {
    rules.iter().position(|r| r.matches(exe_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_and_compiles() {
        let text = Config::default().to_toml();
        let parsed = Config::parse(&text).unwrap();
        assert_eq!(parsed.threshold, 120);
        let rules = parsed.compile_rules().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(find_rule(&rules, "code.EXE"), Some(0));
        assert_eq!(find_rule(&rules, "notepad.exe"), Some(1));
        assert_eq!(rules[1].action, RuleAction::ScreenZoom);
    }

    #[test]
    fn first_matching_rule_wins_and_pass_works() {
        let config = Config::parse(
            r#"
            [[rules]]
            apps = ["chrome.exe"]
            pass = true

            [[rules]]
            apps = ["*"]
            zoom_in = "Win+NumpadAdd"
            zoom_out = "Win+NumpadSubtract"
            cooldown_ms = 100
            "#,
        )
        .unwrap();
        let rules = config.compile_rules().unwrap();
        assert_eq!(
            rules[find_rule(&rules, "chrome.exe").unwrap()].action,
            RuleAction::Pass
        );
        let other = &rules[find_rule(&rules, "excel.exe").unwrap()];
        assert!(matches!(other.action, RuleAction::Zoom { zoom_in, .. } if zoom_in.win));
        assert_eq!(other.settings.cooldown, Duration::from_millis(100));
        assert_eq!(other.settings.threshold, 120); // 全体設定を継承
    }

    #[test]
    fn no_rules_means_nothing_matches() {
        let config = Config::parse("rules = []").unwrap();
        let rules = config.compile_rules().unwrap();
        assert_eq!(find_rule(&rules, "Code.exe"), None);
    }

    #[test]
    fn conflicting_actions_are_rejected() {
        let config = Config::parse(
            r#"
            [[rules]]
            apps = ["*"]
            screen_zoom = true
            zoom_in = "Ctrl+NumpadAdd"
            "#,
        )
        .unwrap();
        assert!(config.compile_rules().is_err());
    }

    #[test]
    fn bad_key_is_reported() {
        let config = Config::parse(
            r#"
            [[rules]]
            apps = ["*"]
            zoom_in = "Ctrl+Nope"
            zoom_out = "Ctrl+NumpadSubtract"
            "#,
        )
        .unwrap();
        let err = config.compile_rules().unwrap_err();
        assert!(err.contains("Nope"), "{err}");
    }
}
