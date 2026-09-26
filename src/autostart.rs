//! Windows へのサインイン時の自動起動 (スタートアップ登録)。
//!
//! `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` に
//! 「名前 = 実行するコマンドライン」の値を置くと、サインイン時に Explorer が実行してくれる。
//! HKCU (現在のユーザー) なので管理者権限は要らない。
//! 「設定 → アプリ → スタートアップ」の一覧にもここに登録したものが表示される。

use std::ffi::c_void;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("pinch-zoom");

/// Run キーに登録するコマンドライン: 空白を含むパスでも動くよう "" で囲む
fn command_line() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

/// この exe が自動起動に登録されているか。
pub fn is_enabled() -> bool {
    let Some(expected) = command_line() else {
        return false;
    };
    let mut buf = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buf) as u32; // バイト数で渡し、書き込まれたバイト数が返る
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ, // 文字列 (REG_SZ) のときだけ読む
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
    };
    if result != ERROR_SUCCESS {
        return false;
    }
    // size は末尾の NUL を含むバイト数
    let len = (size as usize / 2).saturating_sub(1);
    String::from_utf16_lossy(&buf[..len]).eq_ignore_ascii_case(&expected)
}

/// 自動起動を登録 / 解除する。
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let result = unsafe {
        if enabled {
            let Some(cmd) = command_line() else {
                return Err("exe のパスを取得できませんでした".into());
            };
            let data = HSTRING::from(cmd);
            // REG_SZ は NUL 終端込みのバイト数を渡す
            let bytes = (data.len() + 1) * 2;
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE_NAME,
                REG_SZ.0,
                Some(data.as_ptr() as *const c_void),
                bytes as u32,
            )
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME)
        }
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(windows::core::Error::from(result.to_hresult()).message())
    }
}
