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
    pub(crate) fn apply_appearance(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.value("gui.appearance", "system").as_str() {
            "light" => egui::ThemePreference::Light,
            "dark" => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::System,
        });
    }

    pub(super) fn appearance_options(&mut self, ui: &mut egui::Ui) {
        const KEY: &str = "gui.appearance";
        let current = self.value(KEY, "system");
        let mut value = current.as_str();
        theme::card(ui, |ui| {
            theme::setting_row(
                ui,
                "Appearance",
                "Choose the window theme. System follows this machine's light or dark setting.",
                Some(KEY),
                |ui| {
                    if theme::segmented(
                        ui,
                        &mut value,
                        &[("system", "System"), ("light", "Light"), ("dark", "Dark")],
                    )
                    .changed()
                    {
                        self.edit(KEY, value, "system");
                        self.apply_appearance(ui.ctx());
                    }
                },
            );
        });
        self.toggle_card(
            ui,
            None,
            &[Toggle {
                key: "gui.hide_addresses",
                title: "Hide addresses",
                help: "Mask IP addresses and ports in this window.",
                default: "false",
                unavailable: None,
            }],
        );
        self.update_options(ui);
    }

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
                    title: "Hide the cursor on the machine without input",
                    help: "Hide while driving another machine or after remote input leaves. Show it when input returns.",
                    default: "true",
                    unavailable: (!a7n_platform::input::idle_cursor_supported())
                        .then_some(NOT_ON_WAYLAND),
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
        self.local_keys(ui);
    }

    fn local_keys(&mut self, ui: &mut egui::Ui) {
        const KEY: &str = "input.local_keys";
        theme::card(ui, |ui| {
            ui.heading("Local keys and buttons");
            theme::small(
                ui,
                "Keep these inputs on this machine while driving another. Hotkeys still take priority.",
            );
            if !a7n_platform::input::LOCAL_INPUTS_SUPPORTED {
                theme::small(ui, "Local pass-through is unavailable on X11 and Wayland.");
            }
            let mut names: Vec<String> = self
                .value(KEY, "")
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect();
            let mut removed = None;
            for (index, name) in names.iter().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(name);
                        if theme::button(
                            ui,
                            theme::ButtonKind::Ghost,
                            theme::ButtonSize::Sm,
                            None,
                            "Remove",
                        )
                        .clicked()
                        {
                            removed = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = removed {
                names.remove(index);
                self.edit(KEY, &names.join(","), "");
            }
            self.record_hotkey(ui, KEY, a7n_ipc::HotkeyKind::Single);
        });
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
