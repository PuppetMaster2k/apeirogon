// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;

use crate::{App, theme};

/// One on/off setting row; `unavailable` names why it cannot be used here.
struct Toggle {
    key: &'static str,
    title: &'static str,
    help: &'static str,
    default: &'static str,
    unavailable: Option<&'static str>,
}

const NOT_ON_WAYLAND: &str = "Not available on Wayland";

impl App {
    pub(super) fn input_options(&mut self, ui: &mut egui::Ui) {
        self.toggle_card(
            ui,
            None,
            &[
                Toggle {
                    key: "input.natural_scrolling",
                    title: "Natural scrolling",
                    help: "Reverse the scrolling direction on other machines.",
                    default: "false",
                    unavailable: None,
                },
                Toggle {
                    key: "input.hide_idle_cursor",
                    title: "Hide the cursor when remote input leaves",
                    help: "Show it again when the mouse moves or another machine takes control.",
                    default: "true",
                    unavailable: (!a7n_platform::input::idle_cursor_supported())
                        .then_some("Only available on Windows and X11"),
                },
                Toggle {
                    key: "input.fullscreen_hold",
                    title: "Keep the pointer in full-screen apps",
                    help: "Edges don't switch machines while a full-screen app or game has focus. Shortcuts still work.",
                    default: "false",
                    unavailable: (!a7n_platform::fullscreen::supported()).then_some(NOT_ON_WAYLAND),
                },
            ],
        );
    }

    pub(super) fn feedback_options(&mut self, ui: &mut egui::Ui) {
        let feedback_supported = a7n_platform::feedback::supported();
        let unavailable = (!feedback_supported).then_some(NOT_ON_WAYLAND);
        self.toggle_card(
            ui,
            Some("Feedback"),
            &[
                Toggle {
                    key: "feedback.edge",
                    title: "Edge glow",
                    help: "Show a glow along the screen edge as you push toward another machine.",
                    default: "true",
                    unavailable,
                },
                Toggle {
                    key: "feedback.landing",
                    title: "Landing indicator",
                    help: "Briefly mark where the pointer arrives when another machine takes over.",
                    default: "true",
                    unavailable,
                },
                Toggle {
                    key: "feedback.haptics",
                    title: "Trackpad haptics",
                    help: "Tick on the trackpad as the edge push builds up.",
                    default: "false",
                    unavailable: (!a7n_platform::feedback::haptics_supported())
                        .then_some("Only available on macOS"),
                },
            ],
        );
    }

    fn toggle_card(&mut self, ui: &mut egui::Ui, heading: Option<&str>, toggles: &[Toggle]) {
        theme::card(ui, |ui| {
            if let Some(heading) = heading {
                ui.heading(heading);
            }
            for toggle in toggles {
                let mut value = self.value(toggle.key, toggle.default) == "true";
                theme::setting_row(ui, toggle.title, toggle.help, Some(toggle.key), |ui| {
                    ui.add_enabled_ui(toggle.unavailable.is_none(), |ui| {
                        if theme::toggle(ui, &mut value).changed() {
                            let text = if value { "true" } else { "false" };
                            self.edit(toggle.key, text, toggle.default);
                        }
                    });
                    if let Some(reason) = toggle.unavailable {
                        theme::small(ui, reason);
                    }
                });
            }
        });
    }
}
