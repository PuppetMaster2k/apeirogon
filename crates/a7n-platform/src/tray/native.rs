// SPDX-License-Identifier: GPL-3.0-or-later

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use super::Action;

const ICON_SIZE: u32 = 32;
const CHANNELS: u32 = 4;
const CORNER_CUT: u32 = 10;
const POLYGON_RADIUS: u32 = 21;
const ICON_COLOR: [u8; 3] = [65, 145, 220];

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error(transparent)]
    Menu(#[from] tray_icon::menu::Error),
    #[error(transparent)]
    Icon(#[from] tray_icon::BadIcon),
    #[error(transparent)]
    Native(#[from] tray_icon::Error),
}

pub struct Tray {
    _icon: TrayIcon,
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
        let status = MenuItem::new("Disconnected", false, None);
        let focused = MenuItem::new("Focus: unknown", false, None);
        let pause = MenuItem::new("Pause", true, None);
        let settings = MenuItem::new("Settings", true, None);
        let quit = MenuItem::new("Quit", true, None);
        menu.append_items(&[&status, &focused, &pause, &settings, &quit])?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("Apeirogon")
            .with_menu(Box::new(menu))
            .with_icon(Icon::from_rgba(icon_rgba(), ICON_SIZE, ICON_SIZE)?)
            .build()?;
        // The default event channel is bypassed when a handler is installed.
        let (sender, receiver) = std::sync::mpsc::channel();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = sender.send(event);
            wake();
        }));
        Ok(Self {
            _icon: icon,
            status,
            focused,
            pause,
            settings,
            quit,
            events: receiver,
        })
    }

    pub fn update(&self, status: &str, focused: &str, paused: bool) {
        self.status.set_text(status);
        self.focused.set_text(format!("Focus: {focused}"));
        self.pause.set_text(if paused { "Resume" } else { "Pause" });
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

fn icon_rgba() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * CHANNELS) as usize);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let inside = x + y >= CORNER_CUT
                && x + y <= 2 * (ICON_SIZE - 1) - CORNER_CUT
                && x.abs_diff(y) <= POLYGON_RADIUS;
            rgba.extend_from_slice(&ICON_COLOR);
            rgba.push(if inside { u8::MAX } else { 0 });
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_rgba_polygon_has_transparent_corners_and_opaque_center() {
        // Arrange
        let center =
            ((ICON_SIZE / 2 * ICON_SIZE + ICON_SIZE / 2) * CHANNELS + CHANNELS - 1) as usize;

        // Act
        let rgba = icon_rgba();

        // Assert
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * CHANNELS) as usize);
        assert_eq!(rgba[(CHANNELS - 1) as usize], 0);
        assert_eq!(rgba[center], u8::MAX);
    }
}
