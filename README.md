# pinch-zoom

ノートPCのタッチパッドの **ピンチ** を、アプリごとに好きな動作に変換する常駐ツールです。
**Windows 版** と **Mac 版** があります (Mac 版は [Mac 版](#mac-版) を参照)。

Windows 版は既定では次のように動きます。

- **VS Code / Cursor**: エディタの文字だけでなく **ウィンドウ全体をズーム** (`Ctrl+テンキー+` / `Ctrl+テンキー-`)
- **それ以外の全アプリ**: **画面全体をピンチ量に合わせて滑らかに拡大・縮小** (スマホのピンチズームのような動き)。
  ピンチした場所が中心になり、拡大中はカーソルを動かすと表示が追従します。ピンチインで等倍に戻せば終了です。

送るキーやアプリの組み合わせは `config.toml` で自由に変えられます ([設定](#設定-windows))。

Rust + [windows クレート](https://crates.io/crates/windows) で書いています。

- タッチパッドの生データ (Raw Input) から 2 本指の間隔の変化を読み取り、**どのアプリの上でも** ピンチを検知
- 低レベルマウスフック (`SetWindowsHookEx(WH_MOUSE_LL)`) で Ctrl+ホイール (マウスでの Ctrl+ホイールや、Windows がピンチから作るもの) も検知
- フォアグラウンドのアプリに一致するルールがあれば元のイベントを握りつぶし、そのルールのキーを `SendInput` で送る
- 細かいイベントを蓄積し、閾値に達したら 1 段ズーム → クールダウン
- タスクトレイ常駐 (有効/無効, 設定ファイルを開く, 設定を再読み込み, 終了)。ウィンドウは出ません

## インストール (Windows)

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

## Mac 版

Mac では、ピンチしたウィンドウだけを **Chrome のピンチのように** 拡大します。

- **VS Code / Cursor**: ウィンドウ全体をズーム (`⌘+テンキー+` / `⌘+テンキー-` を送ります)
- **Chrome / Safari / Edge / Firefox / Arc / Brave / プレビュー / 写真 / マップ など**: アプリ本来のピンチのまま
- **それ以外の全アプリ**: ピンチしたウィンドウの中身を、ピンチした場所を中心に滑らかに拡大
  - 拡大中は **2 本指スクロールで表示位置を動かせます**。端まで来たら、その先はアプリ自身のスクロールになります
  - 拡大したまま **クリック・ドラッグ・入力ができます** (見えている位置にそのまま当たります)
  - **ピンチインで等倍に戻すと終了**。2 本指のダブルタップ (スマートズーム) でも 2 倍 ⇔ 等倍 を切り替えられます
  - ウィンドウの外をクリックする、別のアプリに切り替える、ダイアログが開く、でも自動で等倍に戻ります

macOS 12.3 以降、Apple シリコン / Intel のどちらでも動きます。

> Mac 版は新しく、まだ実機での確認が十分ではありません。うまく動かないときは Issue で教えてください。

### インストール (Mac)

**ターミナル** を開いて (Spotlight で「ターミナル」と検索)、次の 1 行を貼り付けて Enter:

```sh
curl -fsSL https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/install.sh | sh
```

これだけで次のことが行われます。管理者権限も Rust も要りません。

1. ビルド済みの `pinch-zoom.app` を GitHub Releases からダウンロード
2. `~/Applications/` (ホームフォルダの「アプリケーション」) に配置
3. ログイン時に自動起動するよう登録
4. 起動 (メニューバーに虫眼鏡のアイコンが出ます)

**初回は権限の許可が 2 つ必要です。** macOS のダイアログが出たら「システム設定を開く」を押し、
**システム設定 → プライバシーとセキュリティ** で pinch-zoom をオンにしてください。

| 権限 | 何に使うか |
| --- | --- |
| アクセシビリティ | ピンチを横取りする・クリックの位置を書き換える・キーを送る・ウィンドウを前面に出す |
| 画面収録とシステムオーディオ録音 | 拡大するウィンドウを撮影して、拡大して映す (音声は録りません) |

両方オンにしたら、メニューバーの pinch-zoom のメニューから **「再起動」** を選んでください
(画面収録の許可は、アプリを起動し直すまで有効になりません)。メニューの一番上に「動作中」と出ていれば準備完了です。

- **更新**: 同じ 1 行をもう一度実行 (設定はそのまま残ります)。
  Apple の証明書で署名していないため、macOS は更新のたびに別のアプリとして扱います。
  **更新したら権限の許可をやり直してください** (インストーラが古い許可を消すので、ダイアログに従ってもう一度オンにします)。
- **アンインストール**:
  ```sh
  curl -fsSL https://raw.githubusercontent.com/1000ldk/mouse-custom-own/main/uninstall.sh | sh
  ```
- ログイン時の起動は、メニューの「ログイン時に起動」でも切り替えられます。

### 使い方 (Mac)

メニューバーのアイコンをクリックするとメニューが出ます。

- **有効**: チェックを外すと、ピンチには何もしなくなります (アイコンが `−` の虫眼鏡になります)
- **設定ファイルを開く** / **設定を再読み込み**: 設定ファイル (下記) を編集して反映
- **ログイン時に起動**
- **アクセシビリティの設定を開く… / 画面収録の設定を開く…**: システム設定の該当ページを開く
- **再起動** / **終了**

アイコンが `⚠` のときは、アクセシビリティがまだ許可されていません。

### 設定 (Mac)

設定ファイルは `~/Library/Application Support/pinch-zoom/config.toml` です (初回起動時に作られます)。
書き方は Windows 版と同じですが、次の点が違います。

- `apps` には **バンドル ID** (`com.apple.TextEdit` など) か **アプリ名** を書きます。
  バンドル ID はターミナルで `osascript -e 'id of app "アプリ名"'` を実行すると分かります
- `window_zoom = true`: ピンチしたウィンドウを拡大する (Mac 版の既定の動作)
- キーの修飾キーは `Cmd` (⌘) / `Option` / `Ctrl` / `Shift`

既定の内容:

```toml
screen_zoom_max = 8.0      # 最大倍率
screen_zoom_speed = 400.0  # 倍率を 2 倍にするのに必要なピンチ量 (400 = 指の間隔が 2 倍で表示も 2 倍)

[[rules]]  # VS Code / Cursor: ウィンドウ全体のズーム
apps = ["com.microsoft.VSCode", "com.microsoft.VSCodeInsiders", "com.todesktop.230313mzl4w4u92"]
zoom_in = "Cmd+NumpadAdd"
zoom_out = "Cmd+NumpadSubtract"

[[rules]]  # 自前でピンチズームできるアプリは、アプリ本来の動きに任せる
apps = ["com.google.Chrome", "com.apple.Safari", "com.microsoft.edgemac", "org.mozilla.firefox", "..."]
pass = true

[[rules]]  # それ以外: ピンチしたウィンドウを拡大
apps = ["*"]
window_zoom = true
```

例えばブラウザでも pinch-zoom の拡大を使いたいなら、`pass = true` のルールからそのブラウザを消してください。

### 仕組み (Mac)

```text
 [トラックパッド] ─ピンチ→ macOS が「拡大ジェスチャー」のイベントを作る
       │
       ▼  アプリに届く前に
 CGEventTap (src/mac/tap.rs)
       ├─ ピンチ: ポインタの下のウィンドウのアプリで [[rules]] を選ぶ
       │     ├─ pass        → そのまま通す
       │     ├─ zoom_in/out → ⌘+テンキー± などのキーをそのアプリに送る (CGEventPostToPid)
       │     └─ window_zoom → ウィンドウの拡大 (下記)
       ├─ スクロール (拡大中): 表示位置を動かす。端に着いたらアプリに渡す
       └─ クリック・マウス移動 (拡大中): 「見えている位置」→「本当の位置」に座標を書き換えて通す

 ウィンドウの拡大:
   ScreenCaptureKit で対象のウィンドウだけを撮り続ける (src/mac/capture.rs)
     → 対象のウィンドウにぴったり重ねた、クリックを素通しする透明ウィンドウに拡大して映す (src/mac/overlay.rs)
```

Mac には Windows の `MagSetFullscreenTransform` のような「ウィンドウや画面を拡大表示する」公開 API が無いので、
撮影した映像を自前で拡大して重ねています。クリックは下にある本物のウィンドウに届くよう、
イベントの座標を「拡大表示で見えている位置」から「本当の位置」に書き換えています (計算は `src/view_zoom.rs`)。

| ファイル | 役割 |
| --- | --- |
| `src/mac/mod.rs` | エントリポイント。多重起動防止 → 設定読込 → メニューバー → 権限の確認 → イベントタップ → メッセージループ |
| `src/mac/tap.rs` | イベントタップ。ピンチの行き先の判定、スクロールでの表示位置の移動、クリックの座標の書き換え |
| `src/mac/zoom.rs` | ウィンドウの拡大の開始・終了・更新。ウィンドウの移動や別のアプリへの切り替えの監視 |
| `src/mac/capture.rs` | ScreenCaptureKit で 1 つのウィンドウを撮り続ける |
| `src/mac/overlay.rs` | 拡大表示用の透明ウィンドウ (Core Animation のレイヤーに撮影した映像を映す) |
| `src/mac/window_list.rs` | 画面上のウィンドウの一覧 (位置・持ち主・重なり順) |
| `src/mac/apps.rs` | アプリの名前 (バンドル ID など)、ウィンドウを前面に出す (アクセシビリティ API) |
| `src/mac/input.rs` | キーの組み合わせをアプリに送る |
| `src/mac/menu.rs` | メニューバーのアイコンとメニュー |
| `src/mac/permissions.rs` / `autostart.rs` / `state.rs` | 権限の確認、ログイン時の起動、全体の状態 |
| `src/view_zoom.rs` | ウィンドウの拡大の計算 (ピンチした場所を中心にした倍率、表示位置、座標の変換)。OS 非依存 |

### 注意・制限 (Mac)

- 拡大は撮影した映像を大きくしたものなので、**倍率を上げると文字がぼやけます** (Chrome のように文字をくっきり描き直すことはできません)。
- 表示は約 1 フレーム (1/60 秒) 遅れます。
- 拡大中、アプリのメニュー・右クリックメニュー・ツールチップは拡大されず、本来の位置に表示されます。
- マウスを乗せたときの表示 (ホバー) は、アプリによってずれることがあります。
- 拡大中は、そのウィンドウに重なっていた他のウィンドウが拡大表示の下に隠れます (ウィンドウの外をクリックすると等倍に戻ります)。
- 拡大中はメニューバーに画面収録中のアイコンが出ます。macOS 15 以降は、画面収録を続けて許可するか定期的に確認されることがあります。
- 動画配信サービスなど、録画が禁止されている映像は拡大中に黒く映ります。

## おすすめ設定: Windows の「ピンチ操作でズーム」をオフにする

**設定 → Bluetooth とデバイス → タッチパッド → スクロールとズーム → 「ピンチ操作でズーム」をオフ** にしてください。

pinch-zoom はタッチパッドの生データを直接読んでいるので、この設定をオフにしても動きます。
オンのままだと、Chrome / Edge / エクスプローラー / 写真 など自前でピンチに反応するアプリでは
**アプリ自身のズームと pinch-zoom のズームが同時に起きます** (Raw Input は入力を覗けるだけで、
アプリに届くのを止められないため)。オフにすると、どのアプリでもピンチは pinch-zoom だけが処理します。

アプリ本来のピンチ (ブラウザのページ拡大など) を使いたい場合は、設定をオンのままにして、
そのアプリに `pass = true` のルールを書いてください ([設定](#設定-windows))。

## 仕組み (Windows)

### ピンチの受け取り方は 2 通り

Windows がピンチを「Ctrl+ホイール」に変換してアプリに届けるのは、ピンチを自前で処理しない
古いタイプのアプリ (PowerShell のコンソールなど) だけです。Chrome / Edge / エクスプローラー / Office /
ストアアプリなどは DirectManipulation などでタッチパッドのジェスチャーを直接受け取るので、
Ctrl+ホイールは発生せず、マウスフックでは何も見えません。

そこで pinch-zoom は次の 2 つを併用しています。

1. **タッチパッドの生データ (Raw Input, `src/touchpad.rs`)**: プレシジョンタッチパッドの HID レポートから
   指ごとの座標を読み、2 本指の間隔の変化をピンチとして検出します (`src/touch_pinch.rs`)。
   フォアグラウンドのアプリに関係なく届くので、どのアプリでも動きます。
   間隔が 2 倍になるとホイール量 400 として扱います (既定の `screen_zoom_speed = 400` なら画面も 2 倍)。
2. **マウスフック (Ctrl+ホイール)**: 物理マウスの Ctrl+ホイール用。タッチパッドに 2 本指が触れている間の
   Ctrl+ホイールは Windows がピンチから作ったものなので、二重にズームしないよう握りつぶすだけにします。

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
| `src/touchpad.rs` | Raw Input でタッチパッドの HID レポートを受け取り、指ごとの座標を取り出す | あり |
| `src/touch_pinch.rs` | HID レポートのフレーム化と、2 本指の間隔からのピンチ検出 | **なし** |
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
| `src/platform.rs` | Windows / Mac どちら向けの値 (キーコード、設定の既定値) を使うか | **なし** |

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

### Mac

1. [rustup](https://rustup.rs/) と Xcode のコマンドラインツール (`xcode-select --install`) をインストール
2. ビルドして `.app` にまとめる

```sh
cargo build --release
./macos/bundle.sh
open target/macos/pinch-zoom.app
```

権限はアプリ (`.app`) に対して与えられるので、実行ファイルを直接ターミナルから起動するより、
`.app` にまとめて `open` で起動するのがおすすめです (直接起動すると、ターミナルに権限が必要になります)。

### Windows

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

## 使い方 (Windows)

`pinch-zoom.exe` を起動するとタスクトレイにアイコンが出ます。

- **右クリック**: メニュー (有効 / 設定ファイルを開く / 設定を再読み込み / Windows 起動時に自動実行 / 終了)
- **ダブルクリック**: 有効⇔無効の切り替え (無効中は警告アイコン)

二重に起動しても 2 つ目はすぐ終了します。

## 設定 (Windows)

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

## スタートアップ登録 (Windows)

インストールスクリプトを使った場合は登録済みです。それ以外の場合は、トレイアイコンの右クリックメニューで
**「Windows 起動時に自動実行」** にチェックを入れてください。

これは `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` に exe のパスを書き込んでいるだけで、
「設定 → アプリ → スタートアップ」の一覧からも有効/無効を切り替えられます。
`shell:startup` フォルダに exe のショートカットを置く方法でも構いません (二重に登録しても多重起動防止により 1 つしか動きません)。

## 注意・トラブルシュート (Windows)

- **特定のアプリでアプリ自身のズームも一緒に起きる** → [おすすめ設定](#おすすめ設定-windows-のピンチ操作でズームをオフにする) を参照。
- タッチパッドの生データを読めるのは **プレシジョンタッチパッド** だけです。古いドライバのタッチパッドでは
  従来どおり Ctrl+ホイールとして届くアプリでのみ動きます。

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
