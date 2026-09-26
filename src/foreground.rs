//! フォアグラウンド (いま操作対象になっている) ウィンドウのプロセス名を調べる。
//!
//! 流れ:
//!   GetForegroundWindow        → HWND (ウィンドウのハンドル)
//!   GetWindowThreadProcessId   → そのウィンドウを作ったプロセスの PID
//!   OpenProcess                → PID からプロセスハンドルを得る
//!   QueryFullProcessImageNameW → 実行ファイルのフルパス (C:\...\Code.exe)
//!
//! フックのコールバックは速く返す必要があるので (後述の hook.rs 参照)、
//! 直前に調べた PID と名前をキャッシュし、同じ PID なら API を呼ばない。

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::PWSTR;

#[derive(Default)]
pub struct ForegroundCache {
    last: Option<(u32, String)>,
}

impl ForegroundCache {
    /// フォアグラウンドウィンドウの実行ファイル名 (例: "Code.exe") を返す。
    pub fn exe_name(&mut self) -> Option<&str> {
        let pid = foreground_pid()?;
        let hit = matches!(&self.last, Some((cached, _)) if *cached == pid);
        if !hit {
            let name = exe_name_of_pid(pid)?;
            self.last = Some((pid, name));
        }
        self.last.as_ref().map(|(_, name)| name.as_str())
    }

    pub fn clear(&mut self) {
        self.last = None;
    }
}

fn foreground_pid() -> Option<u32> {
    // SAFETY: どちらも引数検証をする普通の Win32 API。フォアグラウンドが無い (デスクトップ切り替え中など)
    // と null HWND が返るので弾く。
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

fn exe_name_of_pid(pid: u32) -> Option<String> {
    // PROCESS_QUERY_LIMITED_INFORMATION は最小限の権限。管理者権限のプロセスに対しても
    // 通常ユーザー権限から取得できることが多い (取れなければ None → 「対象外」扱い)。
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        // ハンドルは OS のリソースなので、使い終わったら必ず閉じる (C++ の RAII が無い点に注意)
        let _ = CloseHandle(process);
        result.ok()?;

        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let name = path.rsplit('\\').next().unwrap_or(&path).to_string();
        Some(name)
    }
}
