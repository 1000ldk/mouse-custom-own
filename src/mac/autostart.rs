//! Mac へのログイン時の自動起動。
//!
//! `~/Library/LaunchAgents/` に置いた plist (設定ファイル) を、ログイン時に launchd が読んで実行する。
//! ユーザーのフォルダなので管理者権限は要らない。
//! 「システム設定 → 一般 → ログイン項目」の「バックグラウンドでの実行を許可」にも表示される。
//!
//! install.sh も同じ名前・同じ内容の plist を作る。

use std::path::PathBuf;

use objc2_foundation::NSBundle;

const LABEL: &str = "com.github.1000ldk.pinch-zoom";

fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(format!("Library/LaunchAgents/{LABEL}.plist")))
}

/// 起動するコマンド。.app の中から動いているなら `open -a アプリ` で起動する
/// (権限をアプリ (.app) に対して与えるため、アプリとして起動するのが確実)
fn program_arguments() -> Option<Vec<String>> {
    let bundle = NSBundle::mainBundle().bundlePath().to_string();
    if bundle.ends_with(".app") {
        return Some(vec!["/usr/bin/open".into(), "-a".into(), bundle]);
    }
    let exe = std::env::current_exe().ok()?;
    Some(vec![exe.display().to_string()])
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn plist(arguments: &[String]) -> String {
    let arguments: String = arguments
        .iter()
        .map(|a| format!("\n        <string>{}</string>", xml_escape(a)))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>{arguments}
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#
    )
}

/// いま動いているこのアプリが、ログイン時に起動するよう登録されているか
/// (plist が、いまのアプリの場所を起動するものになっているか)
pub fn is_enabled() -> bool {
    let (Some(path), Some(arguments)) = (plist_path(), program_arguments()) else {
        return false;
    };
    let Some(target) = arguments.last() else {
        return false;
    };
    let target = format!("<string>{}</string>", xml_escape(target));
    std::fs::read_to_string(path).is_ok_and(|text| text.contains(&target))
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let path = plist_path().ok_or("ホームフォルダが見つかりません")?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let arguments = program_arguments().ok_or("アプリの場所が分かりません")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, plist(&arguments)).map_err(|e| e.to_string())
}
