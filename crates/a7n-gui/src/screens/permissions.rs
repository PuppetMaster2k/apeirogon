// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{Permission, Request};
use eframe::egui;
use rust_i18n::t;

use crate::{App, Command, theme};
use theme::{BadgeKind, ButtonKind, ButtonSize};

const PERMISSIONS: [(Permission, &str); 2] = [
    (
        Permission::Accessibility,
        "Let paired machines control this Mac's keyboard and mouse.",
    ),
    (
        Permission::InputMonitoring,
        "Read keyboard and mouse input to share with paired machines.",
    ),
];

impl App {
    /// First-run onboarding; the daemon continues by itself once both are granted.
    pub(crate) fn permissions(&mut self, ui: &mut egui::Ui, missing: &[Permission]) {
        self.page_message(ui);
        theme::overline(ui, &t!("Permissions"));
        for (permission, reason) in PERMISSIONS {
            theme::card(ui, |ui| {
                let title = match permission {
                    Permission::Accessibility => &t!("Accessibility"),
                    Permission::InputMonitoring => &t!("Input Monitoring"),
                };
                theme::setting_row(ui, title, &t!(reason), None, |ui| {
                    if !missing.contains(&permission) {
                        theme::badge(ui, BadgeKind::Online, &t!("Granted"));
                    } else if theme::button(
                        ui,
                        if missing.first() == Some(&permission) {
                            ButtonKind::Primary
                        } else {
                            ButtonKind::Secondary
                        },
                        ButtonSize::Md,
                        None,
                        &t!("Open settings"),
                    )
                    .clicked()
                    {
                        self.send(Command::Request(Request::RequestPermission { permission }));
                    }
                });
            });
            ui.add_space(theme::size::ROW_GAP);
        }
        theme::small(
            ui,
            &t!("Add apeirogond with + if missing; setup resumes automatically.",),
        );
    }
}
