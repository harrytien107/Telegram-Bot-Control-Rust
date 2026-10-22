use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

pub enum TrayAction {
    Show,
    Exit,
}

pub struct AppTray {
    _tray: TrayIcon,
    tray_id: tray_icon::TrayIconId,
    show_id: tray_icon::menu::MenuId,
    exit_id: tray_icon::menu::MenuId,
}

impl AppTray {
    pub fn new() -> Result<Self, String> {
        let menu = Menu::new();
        let show = MenuItem::new("Open Telegram Remote Control", true, None);
        let exit = MenuItem::new("Exit", true, None);
        menu.append(&show).map_err(|error| error.to_string())?;
        menu.append(&exit).map_err(|error| error.to_string())?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Telegram Remote Control")
            .with_icon(make_icon()?)
            .build()
            .map_err(|error| error.to_string())?;

        let tray_id = tray.id().clone();
        Ok(Self {
            _tray: tray,
            tray_id,
            show_id: show.id().clone(),
            exit_id: exit.id().clone(),
        })
    }

    pub fn poll(&self) -> Option<TrayAction> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.show_id {
                return Some(TrayAction::Show);
            }
            if event.id == self.exit_id {
                return Some(TrayAction::Exit);
            }
        }

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click {
                    id,
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick {
                    id,
                    button: MouseButton::Left,
                    ..
                } if id == self.tray_id => return Some(TrayAction::Show),
                _ => {}
            }
        }
        None
    }
}

fn make_icon() -> Result<Icon, String> {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let index = (y * size + x) * 4;
            let dx = x as isize - 16;
            let dy = y as isize - 16;
            if dx * dx + dy * dy <= 14 * 14 {
                rgba[index] = 38;
                rgba[index + 1] = 166;
                rgba[index + 2] = 229;
                rgba[index + 3] = 255;
            }
        }
    }
    Icon::from_rgba(rgba, size as u32, size as u32).map_err(|error| error.to_string())
}
