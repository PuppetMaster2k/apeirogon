// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use rust_i18n::t;

use crate::{App, theme};

/// One on/off setting row; `unavailable` names why it cannot be used here.
struct Toggle<'a> {
    key: &'static str,
    title: &'a str,
    help: &'a str,
    default: &'static str,
    unavailable: Option<&'a str>,
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
        theme::overline(ui, &t!("Appearance"));
        theme::card(ui, |ui| {
            theme::setting_row(
                ui,
                &t!("Appearance"),
                &t!("System follows your light or dark theme."),
                Some(KEY),
                |ui| {
                    if theme::segmented(
                        ui,
                        &mut value,
                        &[
                            ("system", &t!("System")),
                            ("light", &t!("Light")),
                            ("dark", &t!("Dark")),
                        ],
                    )
                    .changed()
                    {
                        self.edit(KEY, value, "system");
                        self.apply_appearance(ui.ctx());
                    }
                },
            );
        });
    }

    pub(super) fn general_options(&mut self, ui: &mut egui::Ui) {
        self.language_options(ui);

        self.toggle_card(
            ui,
            None,
            &[Toggle {
                key: "gui.hide_addresses",
                title: &t!("Hide addresses"),
                help: &t!("Hide IP addresses and ports here."),
                default: "false",
                unavailable: None,
            }],
        );
        let summary = if self.value("gui.update_check", "false") == "true" {
            t!("On")
        } else {
            t!("Off")
        };
        theme::accordion(
            ui,
            "updates",
            &t!("Updates"),
            Some(&summary),
            false,
            false,
            |ui| {
                self.update_options(ui);
            },
        );
    }

    fn language_options(&mut self, ui: &mut egui::Ui) {
        const KEY: &str = "gui.language";
        let current = self.value(KEY, "system");
        let mut value = current.as_str();
        let summary = match value {
            "english" => t!("English"),
            "german" => t!("German"),
            _ => t!("System"),
        };
        theme::accordion(
            ui,
            KEY,
            &t!("Language"),
            Some(&summary),
            false,
            false,
            |ui| {
                theme::setting_row(
                    ui,
                    &t!("Language"),
                    &t!("System follows this machine's language."),
                    Some(KEY),
                    |ui| {
                        if theme::segmented(
                            ui,
                            &mut value,
                            &[
                                ("system", &t!("System")),
                                ("english", &t!("English")),
                                ("german", &t!("German")),
                            ],
                        )
                        .changed()
                        {
                            self.edit(KEY, value, "system");
                            crate::set_language(match value {
                                "english" => a7n_ipc::config::Language::English,
                                "german" => a7n_ipc::config::Language::German,
                                _ => a7n_ipc::config::Language::System,
                            });
                        }
                    },
                );
            },
        );
    }

    pub(super) fn input_options(&mut self, ui: &mut egui::Ui) {
        let speed = self.value("input.speed", "1.0");
        theme::accordion(
            ui,
            "input.speed",
            &t!("Pointer speed"),
            Some(&format!("{speed}×")),
            false,
            false,
            |ui| {
                let key = "input.speed";
                let mut speed = self.value(key, "1.0").parse::<f64>().unwrap_or(1.0);
                if theme::slider(
                    ui,
                    &t!("Pointer speed"),
                    &mut speed,
                    a7n_ipc::config::MIN_SPEED..=a7n_ipc::config::MAX_SPEED,
                    None,
                    |value| format!("{value:.2}×"),
                )
                .changed()
                {
                    self.edit(key, &speed.to_string(), "1.0");
                }
            },
        );

        self.toggle_card(
            ui,
            None,
            &[
                Toggle {
                    key: "input.natural_scrolling",
                    title: &t!("Natural scrolling"),
                    help: &t!("Reverse the scrolling direction on other machines."),
                    default: "false",
                    unavailable: None,
                },
                Toggle {
                    key: "input.hide_idle_cursor",
                    title: &t!("Hide idle cursor"),
                    help: &t!("Hide while idle; show when input returns."),
                    default: "true",
                    unavailable: (!a7n_platform::input::idle_cursor_supported())
                        .then_some(&t!(NOT_ON_WAYLAND)),
                },
                Toggle {
                    key: "input.fullscreen_hold",
                    title: &t!("Full-screen hold"),
                    help: &t!("Block edge switching in full-screen apps; hotkeys still work."),
                    default: "false",
                    unavailable: (!a7n_platform::fullscreen::supported())
                        .then_some(&t!(NOT_ON_WAYLAND)),
                },
            ],
        );
        if a7n_platform::input::NOTCH_CROSSING_SUPPORTED {
            self.toggle_card(
                ui,
                None,
                &[Toggle {
                    key: "input.notch_crossing",
                    title: &t!("Notch crossing"),
                    help: &t!("Cross the top edge only through the display notch."),
                    default: "false",
                    unavailable: None,
                }],
            );
        }

        self.local_keys(ui);
    }

    fn local_keys(&mut self, ui: &mut egui::Ui) {
        const KEY: &str = "input.local_keys";
        let summary = super::chord_editor::chord_summary(&self.value(KEY, ""));
        theme::accordion(
            ui,
            KEY,
            &t!("Local inputs"),
            Some(&summary),
            false,
            false,
            |ui| {
                theme::small(ui, &t!("Keep these inputs local; hotkeys take priority."));
                if !a7n_platform::input::LOCAL_INPUTS_SUPPORTED {
                    theme::small(ui, &t!("Unavailable on X11 and Wayland."));
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
                            theme::keycap(ui, name);
                            if theme::button(
                                ui,
                                theme::ButtonKind::Ghost,
                                theme::ButtonSize::Sm,
                                None,
                                &t!("Remove"),
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
            },
        );
    }

    pub(super) fn feedback_options(&mut self, ui: &mut egui::Ui) {
        theme::overline(ui, &t!("Feedback"));
        let feedback_supported = a7n_platform::feedback::supported();
        let unavailable_text = t!(NOT_ON_WAYLAND);
        let unavailable = (!feedback_supported).then_some(unavailable_text.as_ref());
        theme::accordion_group(ui, |ui| {
            for toggle in [
                Toggle {
                    key: "feedback.edge",
                    title: &t!("Edge glow"),
                    help: &t!("Glow as you push toward another machine."),
                    default: "true",
                    unavailable,
                },
                Toggle {
                    key: "feedback.landing",
                    title: &t!("Landing indicator"),
                    help: &t!("Briefly mark the pointer's arrival."),
                    default: "true",
                    unavailable,
                },
                Toggle {
                    key: "feedback.haptics",
                    title: &t!("Trackpad haptics"),
                    help: &t!("Tick as you push against an edge."),
                    default: "false",
                    unavailable: (!a7n_platform::feedback::haptics_supported())
                        .then_some(&t!("Only available on macOS")),
                },
            ] {
                self.toggle_card(ui, None, &[toggle]);
            }
        });
        let edge = self.value("feedback.edge", "true") == "true";
        let landing = self.value("feedback.landing", "true") == "true";
        if edge || landing {
            ui.add_space(theme::size::ROW_GAP);
            theme::card(ui, |ui| {
                ui.add_enabled_ui(feedback_supported, |ui| {
                    for (key, title, default, choices) in [
                        (
                            "feedback.style",
                            &t!("Style"),
                            "glow",
                            &[
                                ("glow", t!("Glow").as_ref()),
                                ("beam", t!("Beam").as_ref()),
                                ("aperture", t!("Aperture").as_ref()),
                                ("sparks", t!("Sparks").as_ref()),
                                ("ripple", t!("Ripple").as_ref()),
                            ][..],
                        ),
                        (
                            "feedback.landing_style",
                            &t!("Landing animation"),
                            "ring",
                            &[
                                ("ring", t!("Ring").as_ref()),
                                ("pulse", t!("Pulse").as_ref()),
                                ("dot", t!("Dot").as_ref()),
                            ][..],
                        ),
                        (
                            "feedback.size",
                            &t!("Size"),
                            "medium",
                            &[
                                ("small", t!("Small").as_ref()),
                                ("medium", t!("Medium").as_ref()),
                                ("large", t!("Large").as_ref()),
                            ][..],
                        ),
                    ] {
                        if (key == "feedback.style" && !edge)
                            || (key == "feedback.landing_style" && !landing)
                        {
                            continue;
                        }
                        let current = self.value(key, default);
                        let mut value = current.as_str();
                        theme::setting_row(ui, title, "", Some(key), |ui| {
                            if theme::segmented(ui, &mut value, choices).changed() {
                                self.edit(key, value, default);
                            }
                        });
                    }
                    self.feedback_color(ui);
                });
            });
        }
    }

    fn toggle_card(&mut self, ui: &mut egui::Ui, heading: Option<&str>, toggles: &[Toggle<'_>]) {
        let enabled = toggles
            .iter()
            .filter(|toggle| self.value(toggle.key, toggle.default) == "true")
            .count();
        let summary = if toggles.len() > 1 {
            t!(
                "%{count} of %{total} on",
                count = enabled,
                total = toggles.len()
            )
        } else if enabled == 1 {
            t!("On")
        } else {
            t!("Off")
        };
        theme::accordion(
            ui,
            toggles[0].key,
            heading.unwrap_or(toggles[0].title),
            Some(&summary),
            false,
            false,
            |ui| self.toggle_rows(ui, toggles),
        );
    }

    fn toggle_rows(&mut self, ui: &mut egui::Ui, toggles: &[Toggle<'_>]) {
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
    }
}
