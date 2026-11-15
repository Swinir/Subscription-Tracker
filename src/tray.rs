//! The system tray icon: when it is on, closing the window only minimizes it and the app keeps
//! running, so reminders keep arriving. The icon offers to bring the window back or quit, and
//! shows the next payment.

use eframe::egui;

/// What a click on the tray icon or its menu asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Open,
    Quit,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod imp {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub const AVAILABLE: bool = true;

    pub struct Tray {
        /// The icon stays in the tray for as long as this lives.
        _icon: TrayIcon,
        next: MenuItem,
        actions: Receiver<Action>,
    }

    /// A purple ring with a dot, drawn here so no image file is needed.
    fn icon_image() -> Result<Icon, String> {
        const SIZE: u32 = 32;
        let [r, g, b, _] = crate::theme::ACCENT.to_array();
        let centre = (SIZE as f32 - 1.0) / 2.0;
        let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let d = ((x as f32 - centre).powi(2) + (y as f32 - centre).powi(2)).sqrt();
                let ring = (d - 9.0).min(14.5 - d);
                let dot = 4.0 - d;
                let cover = (ring.max(dot) + 0.5).clamp(0.0, 1.0);
                rgba.extend_from_slice(&[r, g, b, (cover * 255.0) as u8]);
            }
        }
        Icon::from_rgba(rgba, SIZE, SIZE).map_err(|e| e.to_string())
    }

    impl Tray {
        /// Puts the icon in the tray. `ctx` is woken when it is clicked.
        pub fn new(ctx: &egui::Context) -> Result<Tray, String> {
            let (send, actions) = channel();
            let open = MenuItem::new("Open Subscription Tracker", true, None);
            let next = MenuItem::new("No payments coming up", false, None);
            let quit = MenuItem::new("Quit", true, None);
            let menu = Menu::new();
            menu.append(&open).map_err(|e| e.to_string())?;
            menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
            menu.append(&next).map_err(|e| e.to_string())?;
            menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
            menu.append(&quit).map_err(|e| e.to_string())?;

            let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
            let (menu_send, menu_ctx) = (send.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let action = if event.id == open_id {
                    Action::Open
                } else if event.id == quit_id {
                    Action::Quit
                } else {
                    return;
                };
                let _ = menu_send.send(action);
                menu_ctx.request_repaint();
            }));
            let click_ctx = ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                let opens = matches!(
                    event,
                    TrayIconEvent::DoubleClick { .. }
                        | TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                );
                if opens {
                    let _ = send.send(Action::Open);
                    click_ctx.request_repaint();
                }
            }));

            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(cfg!(target_os = "macos"))
                .with_icon(icon_image()?)
                .with_tooltip("Subscription Tracker")
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Tray { _icon: icon, next, actions })
        }

        /// What the menu says about the next payment.
        pub fn set_next(&self, text: &str) {
            self.next.set_text(text);
        }

        /// The clicks since last asked.
        pub fn poll(&self) -> Vec<Action> {
            self.actions.try_iter().collect()
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
            TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    use super::*;

    pub const AVAILABLE: bool = false;

    pub struct Tray;

    impl Tray {
        pub fn new(_ctx: &egui::Context) -> Result<Tray, String> {
            Err("The tray icon isn't available on this system.".into())
        }
        pub fn set_next(&self, _text: &str) {}
        pub fn poll(&self) -> Vec<Action> {
            Vec::new()
        }
    }
}

pub use imp::{Tray, AVAILABLE};
