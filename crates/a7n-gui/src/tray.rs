// SPDX-License-Identifier: GPL-3.0-or-later

//! Native tray menu and glyph, driven by the daemon status.
use a7n_ipc::{Request, StatusInfo};
use a7n_platform::tray::{Action, State};
use eframe::egui;
use rust_i18n::t;

use crate::App;
use crate::worker::Command;

impl App {
    pub(crate) fn tray(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else {
            return;
        };
        let (status, focused, paused) = self.status.as_ref().map_or(
            (t!("Disconnected"), t!("unknown").into_owned(), false),
            |s| {
                let focused = if s.focused == s.machine {
                    s.name.clone()
                } else {
                    s.peers
                        .iter()
                        .find(|p| p.id == s.focused)
                        .map_or_else(|| s.focused.to_string(), |p| p.name.clone())
                };
                (
                    if s.input.is_some() {
                        t!("Input unavailable")
                    } else {
                        t!("Connected")
                    },
                    focused,
                    s.paused,
                )
            },
        );
        let waiting = self
            .status
            .as_ref()
            .and_then(|s| s.held_clipboard.as_ref())
            .map(|held| t!("Clipboard waiting from %{name}", name = held.name));
        let (state, tooltip) = tray_state(self.status.as_ref());
        if let Err(error) = tray.set_state(state, &tooltip) {
            self.message = t!("Could not update tray: %{error}", error = error).into_owned();
        }
        let tray = self.tray.as_ref().expect("tray checked above");
        tray.update(
            waiting.as_deref().unwrap_or(&status),
            &t!("Focus: %{name}", name = focused),
            &if paused { t!("Resume") } else { t!("Pause") },
            &t!("Settings"),
            &t!("Quit"),
        );
        while let Some(action) = self
            .tray
            .as_ref()
            .and_then(a7n_platform::tray::Tray::action)
        {
            match action {
                Action::Pause => {
                    self.send(Command::Request(Request::SetPaused { paused: !paused }));
                }
                Action::Settings => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Action::Quit => {
                    self.quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if !self.quit && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
}

fn tray_state(status: Option<&StatusInfo>) -> (a7n_platform::tray::State, String) {
    let Some(s) = status else {
        return (
            State::Error,
            t!("Apeirogon: daemon not running").into_owned(),
        );
    };
    if s.input.is_some() {
        return (
            State::Error,
            t!("Apeirogon: input unavailable").into_owned(),
        );
    }
    if s.paused {
        return (State::Paused, t!("Apeirogon: paused").into_owned());
    }
    if s.focused == s.machine {
        return (State::Idle, "Apeirogon".to_owned());
    }
    match s.peers.iter().find(|p| p.id == s.focused) {
        Some(peer) => (
            State::Remote,
            t!("Apeirogon: controlling %{name}", name = peer.name).into_owned(),
        ),
        None => (State::Idle, "Apeirogon".to_owned()),
    }
}
