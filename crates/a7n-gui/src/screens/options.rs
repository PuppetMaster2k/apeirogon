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
        theme::card(ui, |ui| {
            theme::setting_row(
                ui,
                &t!("Appearance"),
                &t!(
                    "Choose the window theme. System follows this machine's light or dark setting."
                ),
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
        self.language_options(ui);
        self.toggle_card(
            ui,
            None,
            &[Toggle {
                key: "gui.hide_addresses",
                title: &t!("Hide addresses"),
                help: &t!("Mask IP addresses and ports in this window."),
                default: "false",
                unavailable: None,
            }],
        );
        self.update_options(ui);
    }

    fn language_options(&mut self, ui: &mut egui::Ui) {
        const KEY: &str = "gui.language";
        let current = self.value(KEY, "system");
        let mut value = current.as_str();
        theme::card(ui, |ui| {
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
        });
    }

    pub(super) fn input_options(&mut self, ui: &mut egui::Ui) {
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
                    title: &t!("Hide the cursor on the machine without input"),
                    help: &t!("Hide while driving another machine or after remote input leaves. Show it when input returns."),
                    default: "true",
                    unavailable: (!a7n_platform::input::idle_cursor_supported())
                        .then_some(&t!(NOT_ON_WAYLAND)),
                },
                Toggle {
                    key: "input.fullscreen_hold",
                    title: &t!("Keep the pointer in full-screen apps"),
                    help: &t!("Edges don't switch machines while a full-screen app or game has focus. Shortcuts still work."),
                    default: "false",
                    unavailable: (!a7n_platform::fullscreen::supported()).then_some(&t!(NOT_ON_WAYLAND)),
                },
            ],
        );
        if a7n_platform::input::NOTCH_CROSSING_SUPPORTED {
            self.toggle_card(
                ui,
                None,
                &[Toggle {
                    key: "input.notch_crossing",
                    title: &t!("Top edge: only through the notch"),
                    help: &t!(
                        "On the notched display, push through the notch to reach the machine above."
                    ),
                    default: "false",
                    unavailable: None,
                }],
            );
        }
        self.local_keys(ui);
    }

    fn local_keys(&mut self, ui: &mut egui::Ui) {
        const KEY: &str = "input.local_keys";
        theme::card(ui, |ui| {
            ui.heading(t!("Local keys and buttons"));
            theme::small(
                ui,
                &t!(
                    "Keep these inputs on this machine while driving another. Hotkeys still take priority.",
                ),
            );
            if !a7n_platform::input::LOCAL_INPUTS_SUPPORTED {
                theme::small(
                    ui,
                    &t!("Local pass-through is unavailable on X11 and Wayland."),
                );
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
        });
    }

    pub(super) fn feedback_options(&mut self, ui: &mut egui::Ui) {
        let feedback_supported = a7n_platform::feedback::supported();
        let unavailable_text = t!(NOT_ON_WAYLAND);
        let unavailable = (!feedback_supported).then_some(unavailable_text.as_ref());
        theme::card(ui, |ui| {
            ui.heading(t!("Feedback"));
            self.toggle_rows(ui, &[
                Toggle {
                    key: "feedback.edge",
                    title: &t!("Edge glow"),
                    help: &t!("Show a glow along the screen edge as you push toward another machine."),
                    default: "true",
                    unavailable,
                },
                Toggle {
                    key: "feedback.landing",
                    title: &t!("Landing indicator"),
                    help: &t!("Briefly mark where the pointer arrives when another machine takes over."),
                    default: "true",
                    unavailable,
                },
                Toggle {
                    key: "feedback.haptics",
                    title: &t!("Trackpad haptics"),
                    help: &t!("Tick on the trackpad as the edge push builds up."),
                    default: "false",
                    unavailable: (!a7n_platform::feedback::haptics_supported())
                        .then_some(&t!("Only available on macOS")),
                },
            ]);
            let edge = self.value("feedback.edge", "true") == "true";
            let landing = self.value("feedback.landing", "true") == "true";
            if edge || landing {
                ui.add_enabled_ui(feedback_supported, |ui| {
                    for (key, title, default, choices) in [
                        (
                            "feedback.style",
                            &t!("Style"),
                            "glow",
                            &[("glow", t!("Glow").as_ref()), ("beam", t!("Beam").as_ref())][..],
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
                            "feedback.color",
                            &t!("Colour"),
                            "accent",
                            &[
                                ("accent", t!("Accent").as_ref()),
                                ("green", t!("Green").as_ref()),
                                ("orange", t!("Orange").as_ref()),
                                ("pink", t!("Pink").as_ref()),
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
                });
            }
        });
    }

    fn toggle_card(&mut self, ui: &mut egui::Ui, heading: Option<&str>, toggles: &[Toggle<'_>]) {
        theme::card(ui, |ui| {
            if let Some(heading) = heading {
                ui.heading(heading);
            }
            self.toggle_rows(ui, toggles);
        });
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
