//! メニューバーのアイコンとメニュー。
//!
//! メニューの項目が選ばれると、項目に設定した「ターゲット」(このファイルで定義したクラスの
//! オブジェクト) の「アクション」(メソッド) が呼ばれる。Objective-C のメソッドとして呼ばれるので、
//! define_class! で NSObject を継承したクラスを作り、そこにメソッドを定義する。
//!
//! メニューを開く直前には、デリゲート (menuNeedsUpdate:) でチェックマークや状態の表示を更新する。

use std::cell::RefCell;
use std::process::Command;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuDelegate,
    NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSBundle, NSString};

use super::state::with_state;
use super::{autostart, permissions, show_alert, zoom};
use crate::config::Config;

/// 状態によって表示を変える項目
struct Items {
    status: Retained<NSMenuItem>,
    enabled: Retained<NSMenuItem>,
    login: Retained<NSMenuItem>,
}

define_class!(
    // SAFETY: NSObject を継承するだけ。Drop も実装しない
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PinchZoomMenuHandler"]
    #[ivars = Items]
    struct MenuHandler;

    unsafe impl NSObjectProtocol for MenuHandler {}

    unsafe impl NSMenuDelegate for MenuHandler {
        /// メニューが開く直前
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, _menu: &NSMenu) {
            self.update_items();
        }
    }

    impl MenuHandler {
        #[unsafe(method(toggleEnabled:))]
        fn toggle_enabled(&self, _sender: Option<&AnyObject>) {
            with_state(|state| {
                state.enabled = !state.enabled;
                state.gesture = None;
                state.pinch.reset();
                if !state.enabled {
                    zoom::end(state);
                }
            });
            refresh();
        }

        #[unsafe(method(openConfig:))]
        fn open_config(&self, _sender: Option<&AnyObject>) {
            if let Some(path) = with_state(|state| state.config_path.clone()) {
                // -t: テキストエディタで開く (.toml に関連付けが無くても開ける)
                let _ = Command::new("open").arg("-t").arg(path).spawn();
            }
        }

        #[unsafe(method(reloadConfig:))]
        fn reload_config(&self, _sender: Option<&AnyObject>) {
            let Some(path) = with_state(|state| state.config_path.clone()) else {
                return;
            };
            match Config::load_or_create(&path) {
                Ok((config, rules)) => {
                    with_state(|state| state.apply_config(&config, rules));
                }
                // ダイアログは with_state の外で出す
                Err(e) => show_alert(self.mtm(), "設定ファイルを読み込めませんでした", &e),
            }
        }

        #[unsafe(method(toggleLogin:))]
        fn toggle_login(&self, _sender: Option<&AnyObject>) {
            if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                show_alert(self.mtm(), "ログイン時の起動を設定できませんでした", &e);
            }
        }

        #[unsafe(method(openAccessibility:))]
        fn open_accessibility(&self, _sender: Option<&AnyObject>) {
            permissions::open_settings("Privacy_Accessibility");
        }

        #[unsafe(method(openScreenRecording:))]
        fn open_screen_recording(&self, _sender: Option<&AnyObject>) {
            permissions::open_settings("Privacy_ScreenCapture");
        }

        /// 権限を変えた後などに。1 秒後に起動し直す
        #[unsafe(method(restart:))]
        fn restart(&self, _sender: Option<&AnyObject>) {
            let bundle = NSBundle::mainBundle().bundlePath().to_string();
            let target = if bundle.ends_with(".app") {
                bundle
            } else {
                std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            };
            // 自分が終わってから起動するよう、別のプロセス (sh) に任せる
            let _ = Command::new("/bin/sh")
                .arg("-c")
                .arg(r#"sleep 1; if [ -d "$0" ]; then open "$0"; else "$0" & fi"#)
                .arg(target)
                .spawn();
            quit(self.mtm());
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            quit(self.mtm());
        }
    }
);

impl MenuHandler {
    fn new(mtm: MainThreadMarker, items: Items) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(items);
        // SAFETY: NSObject の init を呼ぶだけ
        unsafe { msg_send![super(this), init] }
    }

    fn update_items(&self) {
        let items = self.ivars();
        let (enabled, has_tap) =
            with_state(|state| (state.enabled, state.tap.is_some())).unwrap_or((true, false));
        let status = if !has_tap {
            "⚠︎ アクセシビリティの許可を待っています"
        } else if !permissions::screen_capture_allowed() {
            "⚠︎ 画面収録が許可されていません"
        } else if !enabled {
            "停止中"
        } else {
            "動作中"
        };
        items.status.setTitle(&NSString::from_str(status));
        items.enabled.setState(if enabled {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        items.login.setState(if autostart::is_enabled() {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
    }
}

fn quit(mtm: MainThreadMarker) {
    // 拡大中なら撮影を止め、拡大表示を消してから終わる
    with_state(zoom::end);
    NSApplication::sharedApplication(mtm).terminate(None);
}

struct MenuUi {
    status_item: Retained<NSStatusItem>,
    /// メニューのデリゲートとターゲットは弱い参照なので、ここで持っておく
    _handler: Retained<MenuHandler>,
}

thread_local! {
    static MENU: RefCell<Option<MenuUi>> = const { RefCell::new(None) };
}

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
) -> Retained<NSMenuItem> {
    // SAFETY: アクションはハンドラーに定義したメソッドの名前
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    }
}

pub fn install(mtm: MainThreadMarker) {
    let status = item(mtm, "", None, "");
    status.setEnabled(false);
    let enabled = item(mtm, "有効", Some(sel!(toggleEnabled:)), "");
    let login = item(mtm, "ログイン時に起動", Some(sel!(toggleLogin:)), "");
    let actions = [
        item(mtm, "設定ファイルを開く", Some(sel!(openConfig:)), ","),
        item(mtm, "設定を再読み込み", Some(sel!(reloadConfig:)), "r"),
        item(
            mtm,
            "アクセシビリティの設定を開く…",
            Some(sel!(openAccessibility:)),
            "",
        ),
        item(
            mtm,
            "画面収録の設定を開く…",
            Some(sel!(openScreenRecording:)),
            "",
        ),
        item(mtm, "再起動", Some(sel!(restart:)), ""),
        item(mtm, "終了", Some(sel!(quit:)), "q"),
    ];
    let [open, reload, accessibility, screen, restart, quit] = &actions;

    let handler = MenuHandler::new(
        mtm,
        Items {
            status: status.clone(),
            enabled: enabled.clone(),
            login: login.clone(),
        },
    );
    for target in [&enabled, &login].into_iter().chain(actions.iter()) {
        // SAFETY: ハンドラーは MENU で持ち続けるので、メニューが使われている間は有効
        unsafe { target.setTarget(Some(&handler)) };
    }

    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let separator = || NSMenuItem::separatorItem(mtm);
    for entry in [
        &status,
        &separator(),
        &enabled,
        &separator(),
        open,
        reload,
        &login,
        &separator(),
        accessibility,
        screen,
        &separator(),
        restart,
        quit,
    ] {
        menu.addItem(entry);
    }
    menu.setDelegate(Some(ProtocolObject::from_ref(&*handler)));

    let status_item =
        NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    status_item.setMenu(Some(&menu));
    MENU.with(|m| {
        *m.borrow_mut() = Some(MenuUi {
            status_item,
            _handler: handler,
        })
    });
    refresh();
}

/// メニューバーのアイコンを状態に合わせる
pub fn refresh() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let (enabled, has_tap) =
        with_state(|state| (state.enabled, state.tap.is_some())).unwrap_or((true, false));
    // SF Symbols のアイコン名
    let symbol = if !has_tap {
        "exclamationmark.triangle"
    } else if enabled {
        "plus.magnifyingglass"
    } else {
        "minus.magnifyingglass"
    };
    MENU.with(|m| {
        let menu = m.borrow();
        let Some(ui) = menu.as_ref() else {
            return;
        };
        let Some(button) = ui.status_item.button(mtm) else {
            return;
        };
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(symbol),
            Some(&NSString::from_str("pinch-zoom")),
        );
        match image {
            Some(image) => {
                // テンプレート画像にすると、メニューバーの明るさ (ライト / ダーク) に合わせて色が変わる
                image.setTemplate(true);
                button.setImage(Some(&image));
            }
            None => button.setTitle(&NSString::from_str("🔍")),
        }
    });
}
