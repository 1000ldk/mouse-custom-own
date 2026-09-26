# pinch-zoom

ノートPCのタッチパッドで **VS Code 上でピンチ** したとき、エディタの文字だけでなく
**ウィンドウ全体をズーム** (`Ctrl+テンキー+` / `Ctrl+テンキー-`) させる Windows 用の常駐ツールです。
Rust + [windows クレート](https://crates.io/crates/windows) で書いています。

- 低レベルマウスフック (`SetWindowsHookEx(WH_MOUSE_LL)`) でピンチ (= Ctrl+ホイール) を検知
- フォアグラウンドが対象アプリ (既定: `Code.exe`) のときだけ元のイベントを握りつぶし、`SendInput` でズームキーを送る
- 細かいイベントを蓄積し、閾値に達したら 1 段ズーム → クールダウン
- タスクトレイ常駐 (有効/無効, 設定ファイルを開く, 設定を再読み込み, 終了)。ウィンドウは出ません

## 仕組み

### 前提: ピンチは「Ctrl+ホイール」として届く

プレシジョンタッチパッドでピンチすると、Windows は **Ctrl キーを押した状態をシミュレートしつつ、
小さな delta の `WM_MOUSEWHEEL` を大量に** 送ります (ピンチアウト = 正、ピンチイン = 負)。
VS Code はこれを「エディタのフォントズーム」として扱う (`editor.mouseWheelZoom`) か無視するだけで、
ウィンドウ全体のズームはしません。

### 処理の流れ

```text
 [タッチパッド] ─ピンチ→ Ctrl + WM_MOUSEWHEEL (delta 小 × 大量)
       │
       ▼  OS が「フックを登録したスレッド」(= メインスレッド) で呼ぶ
 hook::mouse_proc
       ├─ ホイール以外 / Ctrl 無し / 無効中 / 対象外アプリ → CallNextHookEx (そのまま通す)
       └─ 対象アプリ
            ├─ gesture::PinchTracker に delta を渡す
            │     閾値到達 & クールダウン外 → PostMessage(WM_APP_ZOOM) で自分のウィンドウに依頼
            └─ LRESULT(1) を返して元のイベントを握りつぶす

 main のメッセージループ: GetMessage → DispatchMessage
       ▼
 window::wnd_proc
       ├─ WM_APP_ZOOM → input::send_zoom … SendInput で Ctrl+テンキー± を送る
       ├─ WM_APP_TRAY → トレイのクリック (右クリック: メニュー / ダブルクリック: 有効⇔無効)
       └─ TaskbarCreated → Explorer 再起動時にトレイアイコンを登録し直す
```

ポイント:

- **フックのコールバックは速く返す。** 低レベルフックはシステム全体のマウス入力を止めて待たせているので、
  遅いと Windows にフックを外されます。キー送信は `PostMessage` で後回しにし、コールバックから戻った後に
  メッセージループ経由で行います。
- **メッセージループは必須。** 低レベルフックのコールバックは、登録したスレッドが `GetMessage` などで
  メッセージを待っているときに実行されます。ウィンドウを出さないアプリでもループが要るのはこのためです。
- **Ctrl を離さない。** ピンチ中は Ctrl が押された扱いなので、テンキー± だけを送れば `Ctrl+テンキー±` になります。
  ここで Ctrl の keyup を送るとピンチの残りが普通のスクロールになってしまうので、
  Ctrl が押されていないときだけ Ctrl の down/up を付け足します。
- **スキャンコードも送る。** VS Code (Chromium) はキーの物理位置 (`KeyboardEvent.code`) でキーバインドを判定するので、
  `MapVirtualKeyW` で仮想キーからスキャンコードを求めて `SendInput` に渡しています。

### モジュール構成

| ファイル | 役割 | Win32 依存 |
| --- | --- | --- |
| `src/main.rs` | エントリポイント。多重起動防止 → 設定読込 → 非表示ウィンドウ → トレイ → フック → メッセージループ | あり |
| `src/hook.rs` | `WH_MOUSE_LL` の登録/解除とコールバック。通す/握りつぶすの判定 | あり |
| `src/gesture.rs` | delta の蓄積・閾値・クールダウン・向き反転の純粋ロジック | **なし** (Linux でもテスト可) |
| `src/config.rs` | `config.toml` の読込・既定値生成 | **なし** |
| `src/foreground.rs` | フォアグラウンドウィンドウ → PID → 実行ファイル名 (PID でキャッシュ) | あり |
| `src/input.rs` | `SendInput` でズームキーを送る | あり |
| `src/window.rs` | 非表示ウィンドウ、ウィンドウプロシージャ、タスクトレイとメニュー | あり |
| `src/app.rs` | 全体の状態 (`thread_local!` + `RefCell`)。コールバックから参照する | あり |

各ファイルの先頭と要所に、Win32 の仕組み (フック、メッセージループ、SendInput、ウィンドウプロシージャなど) の
説明コメントを書いています。

### なぜ Rust か

- ランタイム不要の単体 exe (数百 KB) になり、常駐ツールとして軽い。
- `windows` クレートは Win32 API をほぼそのままの名前・引数で公開しているので、
  Microsoft のドキュメント (C/C++ 向け) と 1 対 1 で対応させながら学べる。
- C# でも書けますが、フックのデリゲートが GC で回収されて落ちる、という定番の罠があり、
  WinForms を使うとメッセージループが隠れて仕組みが見えにくくなります。
  Go はコールバックとスレッド (`runtime.LockOSThread` が必須) の扱いに癖があります。
  「Win32 を学ぶ」目的なら Rust か C++ が素直です。

## ビルド

Windows 上で:

1. [rustup](https://rustup.rs/) をインストール (既定の `x86_64-pc-windows-msvc` ツールチェーン)
2. リンカとして Visual Studio Build Tools の「C++ によるデスクトップ開発」をインストール (rustup のインストーラが案内します)
3. ビルド

```powershell
cargo build --release
# => target\release\pinch-zoom.exe
```

- `cargo run` (debug ビルド) はコンソール付きで起動します。release ビルドはコンソールを出しません。
- ロジックのテストは OS を問わず `cargo test` で実行できます。

## 使い方

`pinch-zoom.exe` を起動するとタスクトレイにアイコンが出ます。

- **右クリック**: メニュー (有効 / 設定ファイルを開く / 設定を再読み込み / 終了)
- **ダブルクリック**: 有効⇔無効の切り替え (無効中は警告アイコン)

二重に起動しても 2 つ目はすぐ終了します。

## 設定

初回起動時に exe と同じフォルダに `config.toml` が作られます。編集後、トレイメニューの
「設定を再読み込み」で反映されます。

```toml
threshold = 120          # 蓄積したホイール量がこの値に達したら 1 段ズーム (120 = マウスホイール 1 ノッチ)
cooldown_ms = 400        # 1 段ズームした後、入力を無視する時間 (ミリ秒)
gesture_gap_ms = 500     # 入力がこの時間途切れたら別のピンチとみなし、蓄積をリセット (ミリ秒)
invert = false           # true でズーム方向を反転
target_processes = ["Code.exe", "Cursor.exe", "Code - Insiders.exe"]  # 大文字小文字は区別しない
```

調整の目安:

- 1 回のピンチで 2 段以上ズームしてしまう → `cooldown_ms` を大きく
- 反応が鈍い / 大きくピンチしないと反応しない → `threshold` を小さく
- 長いピンチで続けて何段もズームさせたい → `cooldown_ms` を小さく

exe を書き込み禁止のフォルダ (`C:\Program Files` など) に置いた場合は `config.toml` を作れないので、
既定値で動作します。`%LOCALAPPDATA%\Programs\pinch-zoom\` などユーザーが書き込める場所に置くのがおすすめです。

## スタートアップ登録

どれか 1 つで OK です。

### A. スタートアップフォルダにショートカットを置く (いちばん簡単)

1. `Win + R` → `shell:startup` → Enter
2. 開いたフォルダに `pinch-zoom.exe` のショートカットを作成 (exe を右ドラッグ →「ショートカットをここに作成」)

### B. レジストリの Run キーに登録する

PowerShell で (パスは置いた場所に合わせる):

```powershell
$exe = "$env:LOCALAPPDATA\Programs\pinch-zoom\pinch-zoom.exe"
New-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" `
  -Name "pinch-zoom" -Value "`"$exe`"" -PropertyType String -Force
```

解除: `Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "pinch-zoom"`

どちらの方法も「設定 → アプリ → スタートアップ」で有効/無効を切り替えられます。

## 注意・トラブルシュート

- **VS Code を管理者として実行している場合は動きません。** Windows の UIPI により、通常権限のプロセスから
  管理者権限のウィンドウへ `SendInput` できないためです。本ツールも管理者で起動するか、VS Code を通常権限で起動してください。
- **AutoHotkey の同等スクリプトと同時に動かさないでください。** 両方がフックして、二重にズームしたり片方が何もしなくなったりします。
- ズームのキーは VS Code 既定のキーバインド (`workbench.action.zoomIn` = `Ctrl+NumpadAdd`,
  `workbench.action.zoomOut` = `Ctrl+NumpadSubtract`) を前提にしています。キーバインドを変えている場合は戻してください。
