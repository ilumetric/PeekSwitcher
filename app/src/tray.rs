//! Tray icon: shows the current layout (grey when auto mode is off).
//! Left click opens the window; the menu toggles auto mode or quits.

use std::cell::RefCell;
use std::sync::Arc;

use pek_core::Lang;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::Core;

struct Tray {
    icon: TrayIcon,
    open: MenuItem,
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

fn icon(layout: Option<Lang>, auto: bool) -> Icon {
    decode(match (layout.unwrap_or(Lang::En), auto) {
        (Lang::En, true) => include_bytes!("../icons/tray/en.png"),
        (Lang::En, false) => include_bytes!("../icons/tray/en_off.png"),
        (Lang::Ru, true) => include_bytes!("../icons/tray/ru.png"),
        (Lang::Ru, false) => include_bytes!("../icons/tray/ru_off.png"),
    })
}

fn texts(en: bool) -> [&'static str; 5] {
    if en {
        ["Open PekSwitcher", "Auto mode", "Quit", "auto mode on", "auto mode off"]
    } else {
        ["Открыть PekSwitcher", "Авторежим", "Выход", "авторежим включён", "авторежим выключен"]
    }
}

fn tooltip(en: bool, layout: Option<Lang>, auto: bool) -> String {
    let t = texts(en);
    format!("PekSwitcher · {} · {}", crate::layout_code(layout), if auto { t[3] } else { t[4] })
}

pub fn create(core: Arc<Core>) {
    let s = core.settings();
    let en = s.ui_lang == "en";
    let t = texts(en);
    let open = MenuItem::new(t[0], true, None);
    let auto = CheckMenuItem::new(t[1], true, s.auto_switch, None);
    let quit = MenuItem::new(t[2], true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[&open, &auto, &PredefinedMenuItem::separator(), &quit]);
    let built = TrayIconBuilder::new()
        .with_icon(icon(core.layout(), s.auto_switch))
        .with_tooltip(tooltip(en, core.layout(), s.auto_switch))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build();
    let Ok(tray_icon) = built else { return };

    let (open_id, auto_id, quit_id) = (open.id().clone(), auto.id().clone(), quit.id().clone());
    let c = core.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let (open_id, auto_id, quit_id, c) = (open_id.clone(), auto_id.clone(), quit_id.clone(), c.clone());
        let _ = slint::invoke_from_event_loop(move || {
            if e.id == open_id {
                crate::show_window();
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
    TRAY.with(|t| *t.borrow_mut() = Some(Tray { icon: tray_icon, open, auto, quit }));
}

/// Refreshes icon, tooltip and menu texts (UI thread only).
pub fn update(core: &Core) {
    let s = core.settings();
    let en = s.ui_lang == "en";
    let t = texts(en);
    TRAY.with(|tray| {
        if let Some(tray) = tray.borrow().as_ref() {
            let _ = tray.icon.set_icon(Some(icon(core.layout(), s.auto_switch)));
            let _ = tray.icon.set_tooltip(Some(tooltip(en, core.layout(), s.auto_switch)));
            tray.open.set_text(t[0]);
            tray.auto.set_text(t[1]);
            tray.auto.set_checked(s.auto_switch);
            tray.quit.set_text(t[2]);
        }
    });
}
