# pinch-zoom

ノートPCのタッチパッドの **ピンチ** を、アプリごとに好きなキー操作に変換する Windows 用の常駐ツールです。
既定では次のように動きます。

- **VS Code / Cursor**: エディタの文字だけでなく **ウィンドウ全体をズーム** (`Ctrl+テンキー+` / `Ctrl+テンキー-`)
- **それ以外の全アプリ**: **画面全体をピンチ量に合わせて滑らかに拡大・縮小** (スマホのピンチズームのような動き)。
  ピンチした場所が中心になり、拡大中はカーソルを動かすと表示が追従します。ピンチインで等倍に戻せば終了です。

送るキーやアプリの組み合わせは `config.toml` で自由に変えられます ([設定](#設定))。

Rust + [windows クレート](https://crates.io/crates/windows) で書いています。

- 低レベルマウスフック (`SetWindowsHookEx(WH_MOUSE_LL)`) でピンチ (= Ctrl+ホイール) を検知
- フォアグラウンドのアプリに一致するルールがあれば元のイベントを握りつぶし、そのルールのキーを `SendInput` で送る
- 細かいイベントを蓄積し、閾値に達したら 1 段ズーム → クールダウン
- タスクトレイ常駐 (有効/無効, 設定ファイルを開く, 設定を再読み込み, 終了)。ウィンドウは出ません

## インストール (かんたん)

**PowerShell** を開いて (スタートメニューで「PowerShell」と検索)、次の 1 行を貼り付けて Enter:

```powershell
irm https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/install.ps1 | iex
```

これだけで次のことが行われます。管理者権限も Rust も要りません。

1. ビルド済みの `pinch-zoom.exe` を GitHub Releases からダウンロード
2. `%LOCALAPPDATA%\Programs\pinch-zoom\` に配置
3. Windows サインイン時に自動起動するよう登録
4. 起動 (タスクトレイにアイコンが出ます)

- **更新**: 同じ 1 行をもう一度実行 (`config.toml` はそのまま残ります)
- **アンインストール**:
  ```powershell
  irm https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/uninstall.ps1 | iex
  ```
- 自動起動はトレイメニューの「Windows 起動時に自動実行」でも切り替えられます。
- exe は main ブランチに push されるたびに GitHub Actions (`.github/workflows/release.yml`) が自動でビルドし、
  [Releases の latest](https://github.com/1000ldk/mouse-custom-own/releases/latest) に置いています。
  ブラウザから直接ダウンロードして使うこともできます (その場合は SmartScreen の警告が出たら「詳細情報 → 実行」)。

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
       ├─ ホイール以外 / Ctrl 無し / 無効中 / 一致するルール無し / pass ルール → CallNextHookEx (そのまま通す)
       └─ フォアグラウンドのアプリに一致するルールあり (config.toml の [[rules]] を上から順に照合)
            ├─ キーを送るルール: gesture::PinchTracker に delta を渡す
            │     閾値到達 & クールダウン外 → ルールのキーを PostMessage(WM_APP_SEND_KEYS) で自分のウィンドウに依頼
            ├─ screen_zoom ルール (または画面ズーム中): delta をそのまま倍率に反映
            │     → PostMessage(WM_APP_UPDATE_SCREEN_ZOOM)  ※マウス移動時も表示位置更新のために投函
            └─ LRESULT(1) を返して元のイベントを握りつぶす

 main のメッセージループ: GetMessage → DispatchMessage
       ▼
 window::wnd_proc
       ├─ WM_APP_SEND_KEYS → input::send_combo … SendInput でキーを送る
       ├─ WM_APP_UPDATE_SCREEN_ZOOM → magnifier … MagSetFullscreenTransform で画面全体の倍率・表示位置を設定
       ├─ WM_APP_TRAY → トレイのクリック (右クリック: メニュー / ダブルクリック: 有効⇔無効)
       └─ TaskbarCreated → Explorer 再起動時にトレイアイコンを登録し直す
```

ポイント:

- **フックのコールバックは速く返す。** 低レベルフックはシステム全体のマウス入力を止めて待たせているので、
  遅いと Windows にフックを外されます。キー送信は `PostMessage` で後回しにし、コールバックから戻った後に
  メッセージループ経由で行います。
- **メッセージループは必須。** 低レベルフックのコールバックは、登録したスレッドが `GetMessage` などで
  メッセージを待っているときに実行されます。ウィンドウを出さないアプリでもループが要るのはこのためです。
- **修飾キーは差分だけ操作する。** ピンチ中は Ctrl が押された扱いです。
  - `Ctrl+テンキー+` を送るときは Ctrl に触らずテンキー+ だけ送ります。Ctrl の keyup を送ると、
    ピンチの残りが普通のスクロールになってしまうためです。
  - `Win+テンキー+` のように Ctrl を含まないキーを送るときは、そのままだと `Ctrl+Win+テンキー+` になるので、
    一時的に Ctrl を離し、送った後に押し直します。
- **スキャンコードも送る。** VS Code (Chromium) はキーの物理位置 (`KeyboardEvent.code`) でキーバインドを判定するので、
  `MapVirtualKeyW` で仮想キーからスキャンコードを求めて `SendInput` に渡しています
  (Win キーや矢印キーなどの「拡張キー」には `KEYEVENTF_EXTENDEDKEY` も付けます)。

### モジュール構成

| ファイル | 役割 | Win32 依存 |
| --- | --- | --- |
| `src/main.rs` | エントリポイント。多重起動防止 → 設定読込 → 非表示ウィンドウ → トレイ → フック → メッセージループ | あり |
| `src/hook.rs` | `WH_MOUSE_LL` の登録/解除とコールバック。通す/握りつぶすの判定 | あり |
| `src/gesture.rs` | delta の蓄積・閾値・クールダウン・向き反転の純粋ロジック | **なし** (Linux でもテスト可) |
| `src/config.rs` | `config.toml` の読込・既定値生成・ルールの解析と照合 | **なし** |
| `src/keys.rs` | `"Ctrl+NumpadAdd"` のようなキー文字列 → 仮想キーコード | **なし** |
| `src/foreground.rs` | フォアグラウンドウィンドウ → PID → 実行ファイル名 (PID でキャッシュ) | あり |
| `src/input.rs` | `SendInput` で任意のキーの組み合わせを送る | あり |
| `src/screen_zoom.rs` | 画面ズームの倍率計算と、カーソル位置からの表示位置計算 | **なし** |
| `src/magnifier.rs` | Magnification API (`MagSetFullscreenTransform`) の呼び出し | あり |
| `src/autostart.rs` | サインイン時の自動起動 (レジストリの Run キー) の登録/解除 | あり |
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

## ソースからビルドする (開発者向け)

Windows 上で:

1. [rustup](https://rustup.rs/) をインストール (既定の `x86_64-pc-windows-msvc` ツールチェーン)
2. リンカとして Visual Studio Build Tools の「C++ によるデスクトップ開発」をインストール (rustup のインストーラが案内します)
3. ビルド

```powershell
cargo build --release
# => target\release\pinch-zoom.exe
```

- 自分でビルドした exe をインストールするには:
  `powershell -ExecutionPolicy Bypass -File .\install.ps1 -ExePath target\release\pinch-zoom.exe`
- `cargo run` (debug ビルド) はコンソール付きで起動します。release ビルドはコンソールを出しません。
- ロジックのテストは OS を問わず `cargo test` で実行できます。

## 使い方

`pinch-zoom.exe` を起動するとタスクトレイにアイコンが出ます。

- **右クリック**: メニュー (有効 / 設定ファイルを開く / 設定を再読み込み / Windows 起動時に自動実行 / 終了)
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

# ルールは上から順に調べ、フォアグラウンドのアプリに最初に一致したものを使う
[[rules]]
apps = ["Code.exe", "Cursor.exe"]     # 実行ファイル名 (大文字小文字は区別しない)
zoom_in = "Ctrl+NumpadAdd"            # ピンチアウトで送るキー
zoom_out = "Ctrl+NumpadSubtract"      # ピンチインで送るキー

[[rules]]
apps = ["*"]                          # "*" = 上のどれにも一致しなかった全アプリ
screen_zoom = true                    # 画面全体を連続ズーム
```

画面ズームの調整 (ファイルの先頭に書く):

```toml
screen_zoom_max = 8.0      # 最大倍率
screen_zoom_speed = 400.0  # 倍率を 2 倍にするのに必要なピンチ量。小さいほど少しのピンチで大きく拡大
```

### ルールの書き方

| キー | 意味 |
| --- | --- |
| `apps` | 実行ファイル名のリスト。`"*"` は全アプリ。タスクマネージャーの「詳細」タブで名前を確認できます |
| `zoom_in` / `zoom_out` | ピンチアウト / ピンチインで送るキー |
| `screen_zoom = true` | キーを送る代わりに画面全体を連続ズームする |
| `pass = true` | 何もせずアプリにそのまま渡す (ブラウザなど、アプリ本来のピンチズームを使いたいとき) |
| `threshold` / `cooldown_ms` | そのルールだけ全体の値を上書き |

キーは `修飾キー+キー` の形で書きます (大文字小文字は区別しない)。

- 修飾キー: `Ctrl`, `Shift`, `Alt`, `Win`
- キー: `A`〜`Z`, `0`〜`9`, `F1`〜`F24`, `Numpad0`〜`Numpad9`, `NumpadAdd`, `NumpadSubtract`,
  `NumpadMultiply`, `NumpadDivide`, `Plus`, `Minus`, `Up`/`Down`/`Left`/`Right`, `PageUp`, `PageDown`,
  `Home`, `End`, `Insert`, `Delete`, `Space`, `Enter`, `Tab`, `Esc`, `Backspace`
- 一覧に無いキーは仮想キーコードで直接指定: `Ctrl+vk:0xBB`
  ([仮想キーコード一覧](https://learn.microsoft.com/windows/win32/inputdev/virtual-key-codes))

例:

```toml
# ブラウザはアプリ本来のピンチズームを使う ("*" のルールより上に書く)
[[rules]]
apps = ["chrome.exe", "msedge.exe", "firefox.exe"]
pass = true

# Ctrl+Plus / Ctrl+Minus でズームするアプリに、そのショートカットを送る例
[[rules]]
apps = ["SomeApp.exe"]
zoom_in = "Ctrl+Plus"
zoom_out = "Ctrl+Minus"
cooldown_ms = 200
```

画面ズームを使わず VS Code だけで動かしたい場合は、`apps = ["*"]` のルールを削除してください。

調整の目安:

- 1 回のピンチで 2 段以上ズームしてしまう → `cooldown_ms` を大きく
- 反応が鈍い / 大きくピンチしないと反応しない → `threshold` を小さく
- 長いピンチで続けて何段もズームさせたい → `cooldown_ms` を小さく

exe を書き込み禁止のフォルダ (`C:\Program Files` など) に置いた場合は `config.toml` を作れないので、
既定値で動作します。`%LOCALAPPDATA%\Programs\pinch-zoom\` などユーザーが書き込める場所に置くのがおすすめです。

## スタートアップ登録

インストールスクリプトを使った場合は登録済みです。それ以外の場合は、トレイアイコンの右クリックメニューで
**「Windows 起動時に自動実行」** にチェックを入れてください。

これは `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` に exe のパスを書き込んでいるだけで、
「設定 → アプリ → スタートアップ」の一覧からも有効/無効を切り替えられます。
`shell:startup` フォルダに exe のショートカットを置く方法でも構いません (二重に登録しても多重起動防止により 1 つしか動きません)。

## 注意・トラブルシュート

- **VS Code を管理者として実行している場合は動きません。** Windows の UIPI により、通常権限のプロセスから
  管理者権限のウィンドウへ `SendInput` できないためです。本ツールも管理者で起動するか、VS Code を通常権限で起動してください。
- **AutoHotkey の同等スクリプトと同時に動かさないでください。** 両方がフックして、二重にズームしたり片方が何もしなくなったりします。
- **画面ズーム**は Windows の Magnification API (標準の拡大鏡と同じ仕組み) を使っています。
  - 拡大中は、どのアプリの上でピンチしても画面ズームの操作になります (VS Code に切り替えても戻せるように)。
  - Windows の拡大鏡 (`Win++`) と同時には使えません。拡大鏡を起動している場合は `Win+Esc` で終了してください。
  - 拡大したまま操作に困ったら、トレイアイコンをダブルクリック (無効化) すると等倍に戻ります。
  - 表示位置の計算は主モニター基準です。複数モニター環境での動作は未確認です。
- VS Code 用のキーは VS Code 既定のキーバインド (`workbench.action.zoomIn` = `Ctrl+NumpadAdd`,
  `workbench.action.zoomOut` = `Ctrl+NumpadSubtract`) を前提にしています。キーバインドを変えている場合は戻してください。
