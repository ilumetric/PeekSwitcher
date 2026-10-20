//! Tray icon: shows the current layout (grey when auto mode is off, an outline when the app is
//! turned off). Left click opens the window; the menu turns the app or auto mode on and off, or quits.

use std::cell::RefCell;
use std::sync::Arc;

use pek_core::{Lang, Settings};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::Core;

struct Tray {
    icon: TrayIcon,
    open: MenuItem,
    enabled: CheckMenuItem,
    auto: CheckMenuItem,
    quit: MenuItem,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

fn decode(bytes: &[u8]) -> Icon {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("tray png");
    let mut buf = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut buf).expect("tray png frame");
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).expect("tray icon")
}

fn icon(layout: Option<Lang>, s: &Settings) -> Icon {
    decode(match (layout.unwrap_or(Lang::En), s.enabled, s.auto_switch) {
        (Lang::En, false, _) => include_bytes!("../icons/tray/en_disabled.png"),
        (Lang::En, true, true) => include_bytes!("../icons/tray/en.png"),
        (Lang::En, true, false) => include_bytes!("../icons/tray/en_off.png"),
        (Lang::Ru, false, _) => include_bytes!("../icons/tray/ru_disabled.png"),
        (Lang::Ru, true, true) => include_bytes!("../icons/tray/ru.png"),
        (Lang::Ru, true, false) => include_bytes!("../icons/tray/ru_off.png"),
    })
}

fn texts(en: bool) -> [&'static str; 7] {
    if en {
        ["Open PeekSwitcher", "Enabled", "Auto mode", "Quit", "auto mode on", "auto mode off", "turned off"]
    } else {
        ["Открыть PeekSwitcher", "Включён", "Авторежим", "Выход", "авторежим включён", "авторежим выключен", "отключён"]
    }
}

fn tooltip(en: bool, layout: Option<Lang>, s: &Settings) -> String {
    let t = texts(en);
    let state = if !s.enabled {
        t[6]
    } else if s.auto_switch {
        t[4]
    } else {
        t[5]
    };
    format!("PeekSwitcher · {} · {state}", crate::layout_code(layout))
}

pub fn create(core: Arc<Core>) {
    let s = core.settings();
    let en = s.ui_lang == "en";
    let t = texts(en);
    let open = MenuItem::new(t[0], true, None);
    let enabled = CheckMenuItem::new(t[1], true, s.enabled, None);
    let auto = CheckMenuItem::new(t[2], s.enabled, s.auto_switch, None);
    let quit = MenuItem::new(t[3], true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[&open, &PredefinedMenuItem::separator(), &enabled, &auto, &PredefinedMenuItem::separator(), &quit]);
    let built = TrayIconBuilder::new()
        .with_icon(icon(core.layout(), &s))
        .with_tooltip(tooltip(en, core.layout(), &s))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build();
    let Ok(tray_icon) = built else { return };

    let ids = (open.id().clone(), enabled.id().clone(), auto.id().clone(), quit.id().clone());
    let c = core.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let ((open_id, enabled_id, auto_id, quit_id), c) = (ids.clone(), c.clone());
        let _ = slint::invoke_from_event_loop(move || {
            if e.id == open_id {
                crate::show_window();
            } else if e.id == enabled_id {
                crate::toggle_enabled(&c);
            } else if e.id == auto_id {
                crate::toggle_auto(&c);
            } else if e.id == quit_id {
                crate::quit(&c);
            }
        });
    }));
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            let _ = slint::invoke_from_event_loop(crate::show_window);
        }
    }));
    TRAY.with(|t| *t.borrow_mut() = Some(Tray { icon: tray_icon, open, enabled, auto, quit }));
}

/// Refreshes icon, tooltip and menu texts (UI thread only).
pub fn update(core: &Core) {
    let s = core.settings();
    let en = s.ui_lang == "en";
    let t = texts(en);
    TRAY.with(|tray| {
        if let Some(tray) = tray.borrow().as_ref() {
            let _ = tray.icon.set_icon(Some(icon(core.layout(), &s)));
            let _ = tray.icon.set_tooltip(Some(tooltip(en, core.layout(), &s)));
            tray.open.set_text(t[0]);
            tray.enabled.set_text(t[1]);
            tray.enabled.set_checked(s.enabled);
            tray.auto.set_text(t[2]);
            tray.auto.set_checked(s.auto_switch);
            tray.auto.set_enabled(s.enabled);
            tray.quit.set_text(t[3]);
        }
    });
}
