// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{Permission, Request};
use eframe::egui;

use crate::{App, Command, theme};
use theme::{BadgeKind, ButtonKind, ButtonSize};

const PERMISSIONS: [(Permission, &str); 2] = [
    (
        Permission::Accessibility,
        "Lets Apeirogon move the pointer and type on this Mac when another machine drives it.",
    ),
    (
        Permission::InputMonitoring,
        "Lets Apeirogon see your keyboard and mouse so it can send them to another machine when the pointer crosses an edge.",
    ),
];

impl App {
    /// First-run onboarding; the daemon continues by itself once both are granted.
    pub(crate) fn permissions(&mut self, ui: &mut egui::Ui, missing: &[Permission]) {
        theme::page_header(
            ui,
            "Allow Apeirogon to share your keyboard and mouse",
            "macOS asks you to approve each permission once.",
            |_| {},
        );
        self.page_message(ui);
        for (permission, reason) in PERMISSIONS {
            theme::card(ui, |ui| {
                theme::setting_row(ui, &permission.to_string(), reason, None, |ui| {
                    if !missing.contains(&permission) {
                        theme::badge(ui, BadgeKind::Online, "Granted");
                    } else if theme::button(
                        ui,
                        ButtonKind::Primary,
                        ButtonSize::Md,
                        None,
                        "Open System Settings",
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
            "After you switch it on, Apeirogon continues automatically. If apeirogond is not in the list, add it with the + button.",
        );
    }
}
