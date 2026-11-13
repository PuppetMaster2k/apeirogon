// SPDX-License-Identifier: GPL-3.0-or-later

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use super::icon::{self, IconError, Variant};
use super::{Action, State};

// macOS menu bar icons must be template images (docs/ui.md section 6).
const TEMPLATE: bool = cfg!(target_os = "macos");

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error(transparent)]
    Menu(#[from] tray_icon::menu::Error),
    #[error(transparent)]
    Icon(#[from] IconError),
    #[error(transparent)]
    Native(#[from] tray_icon::Error),
}

pub struct Tray {
    icon: TrayIcon,
    shown: std::cell::RefCell<(Variant, String)>,
    status: MenuItem,
    focused: MenuItem,
    pause: MenuItem,
    settings: MenuItem,
    quit: MenuItem,
    events: std::sync::mpsc::Receiver<MenuEvent>,
}

impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tray").finish_non_exhaustive()
    }
}

impl Tray {
    /// Create the tray on the window event-loop thread.
    /// # Errors
    /// Returns native menu or icon creation errors.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Result<Self, TrayError> {
        let menu = Menu::new();
        let status = MenuItem::new("", false, None);
        let focused = MenuItem::new("", false, None);
        let pause = MenuItem::new("", true, None);
        let settings = MenuItem::new("", true, None);
        let quit = MenuItem::new("", true, None);
        menu.append_items(&[&status, &focused, &pause, &settings, &quit])?;
        let shown = Variant::current(State::Error);
        let icon = TrayIconBuilder::new()
            .with_tooltip("Apeirogon")
            .with_menu(Box::new(menu))
            .with_icon(icon::load(shown)?)
            .with_icon_as_template(TEMPLATE)
            .build()?;
        // The default event channel is bypassed when a handler is installed.
        let (sender, receiver) = std::sync::mpsc::channel();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = sender.send(event);
            wake();
        }));
        Ok(Self {
            icon,
            shown: std::cell::RefCell::new((shown, "Apeirogon".to_owned())),
            status,
            focused,
            pause,
            settings,
            quit,
            events: receiver,
        })
    }

    pub fn update(&self, status: &str, focused: &str, pause: &str, settings: &str, quit: &str) {
        self.status.set_text(status);
        self.focused.set_text(focused);
        self.pause.set_text(pause);
        self.settings.set_text(settings);
        self.quit.set_text(quit);
    }

    /// Show the glyph for `state`; also follows Windows taskbar theme changes.
    /// # Errors
    /// Returns native icon errors.
    pub fn set_state(&self, state: State, tooltip: &str) -> Result<(), TrayError> {
        let variant = Variant::current(state);
        let mut shown = self.shown.borrow_mut();
        if shown.1 != tooltip {
            self.icon.set_tooltip(Some(tooltip))?;
            tooltip.clone_into(&mut shown.1);
        }
        if shown.0 != variant {
            let icon = Some(icon::load(variant)?);
            // tray-icon ignores set_icon_with_as_template outside macOS.
            if TEMPLATE {
                self.icon.set_icon_with_as_template(icon, true)?;
            } else {
                self.icon.set_icon(icon)?;
            }
            shown.0 = variant;
        }
        Ok(())
    }

    #[must_use]
    pub fn action(&self) -> Option<Action> {
        let event = self.events.try_recv().ok()?;
        if event.id == *self.pause.id() {
            Some(Action::Pause)
        } else if event.id == *self.settings.id() {
            Some(Action::Settings)
        } else if event.id == *self.quit.id() {
            Some(Action::Quit)
        } else {
            None
        }
    }
}
