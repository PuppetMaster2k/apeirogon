// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;

use super::{App, Command, Screen, theme};
use theme::{BadgeKind, ButtonKind, ButtonSize, IconButtonKind, icon};

mod hotkeys;
mod machines;
mod peers;
mod permissions;

impl App {
    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        let (title, subtitle) = if self.screen == Screen::Hotkeys {
            (
                "Hotkeys",
                "Shortcuts for returning home and moving input between machines.",
            )
        } else {
            (
                "Peers & policies",
                "What each paired machine is allowed to do.",
            )
        };
        self.settings_header(ui, title, subtitle);
        self.page_message(ui);
        if self.config.is_none() {
            theme::empty_card(
                ui,
                icon::ARROW_CLOCKWISE,
                "Config is unavailable.",
                "Reload config to try again.",
            );
            return;
        }
        if self.screen == Screen::Hotkeys {
            self.hotkeys(ui);
        } else {
            self.peers(ui);
        }
        if let Some(error) = self.config_validation_error() {
            theme::error(ui, &error);
        }
        ui.add_space(theme::size::SECTION_GAP);
        theme::small(
            ui,
            "Saving rewrites config.toml without comments, like a7n config set.",
        );
    }

    fn settings_header(&mut self, ui: &mut egui::Ui, title: &str, subtitle: &str) {
        theme::page_header(ui, title, subtitle, |ui| {
            ui.add_enabled_ui(
                self.config.is_some()
                    && !self.edits.is_empty()
                    && self.config_validation_error().is_none(),
                |ui| {
                    if theme::button(
                        ui,
                        ButtonKind::Primary,
                        ButtonSize::Md,
                        Some(icon::FLOPPY_DISK),
                        "Save changes",
                    )
                    .clicked()
                    {
                        self.send(Command::SaveConfig(self.edits.clone()));
                    }
                },
            );
            if theme::icon_button(
                ui,
                IconButtonKind::Bordered,
                icon::ARROW_CLOCKWISE,
                "Discard edits and reload config",
                false,
            )
            .clicked()
            {
                self.send(Command::Config);
            }
            if !self.edits.is_empty() {
                theme::badge(ui, BadgeKind::Warning, "Unsaved changes");
            }
        });
    }

    fn config_validation_error(&self) -> Option<String> {
        let mut config = self.config.clone()?;
        config
            .set_many(
                self.edits
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .err()
            .map(|error| error.to_string())
    }

    fn value(&self, key: &str, default: &str) -> String {
        self.edits
            .iter()
            .find(|(edit, _)| edit == key)
            .map(|(_, value)| value.clone())
            .or_else(|| self.config.as_ref().and_then(|config| config.get(key)))
            .unwrap_or_else(|| default.to_owned())
    }

    fn edit(&mut self, key: &str, value: &str, default: &str) {
        let original = self
            .config
            .as_ref()
            .and_then(|config| config.get(key))
            .unwrap_or_else(|| default.to_owned());
        update_edits(&mut self.edits, key, value, &original);
    }

    fn page_message(&self, ui: &mut egui::Ui) {
        if !self.message.is_empty() {
            theme::small(ui, &self.message);
            ui.add_space(theme::size::ROW_GAP);
        }
    }

    pub(super) fn daemon_unavailable(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(theme::size::SECTION_GAP);
            ui.set_max_width(480.0);
            theme::card(ui, |ui| {
                ui.vertical_centered(|ui| {
                    theme::icon_tile(ui, icon::POWER);
                    ui.heading("Daemon isn't running");
                    ui.label("Start the daemon to manage machines and share input.");
                    if let Some(error) = &self.daemon_error {
                        ui.label(
                            egui::RichText::new(error)
                                .font(theme::font_mono(12.0))
                                .color(theme::Palette::of(ui.ctx()).text3),
                        );
                    }
                    self.page_message(ui);
                    if theme::button(
                        ui,
                        ButtonKind::Primary,
                        ButtonSize::Md,
                        Some(icon::POWER),
                        "Start daemon",
                    )
                    .clicked()
                    {
                        self.send(Command::Start);
                    }
                });
            });
        });
    }

    pub(super) fn layout_screen(&mut self, ui: &mut egui::Ui) {
        let mut save = false;
        theme::page_header(
            ui,
            "Layout",
            "Arrange machines to match their positions on your desk.",
            |ui| {
                ui.add_enabled_ui(
                    self.layout
                        .as_ref()
                        .is_some_and(super::canvas::Canvas::is_valid),
                    |ui| {
                        save = theme::button(
                            ui,
                            ButtonKind::Primary,
                            ButtonSize::Md,
                            Some(icon::FLOPPY_DISK),
                            "Save layout",
                        )
                        .clicked();
                    },
                );
                if theme::button(
                    ui,
                    ButtonKind::Ghost,
                    ButtonSize::Md,
                    Some(icon::ARROW_CLOCKWISE),
                    "Reload",
                )
                .on_hover_text("Discard edits and reload layout")
                .clicked()
                {
                    self.send(Command::Request(a7n_ipc::Request::GetLayout));
                }
                if theme::button(
                    ui,
                    ButtonKind::Secondary,
                    ButtonSize::Md,
                    None,
                    "Fit to canvas",
                )
                .clicked()
                    && let Some(canvas) = &mut self.layout
                {
                    canvas.fit();
                }
            },
        );
        self.page_message(ui);
        if let Some(canvas) = &mut self.layout {
            let p = theme::Palette::of(ui.ctx());
            ui.scope(|ui| {
                ui.visuals_mut().widgets.noninteractive.bg_fill = p.bg;
                theme::card(ui, |ui| canvas.show(ui));
            });
            if save && canvas.is_valid() {
                let placed = canvas.placed();
                self.send(Command::Request(a7n_ipc::Request::SetLayout { placed }));
            }
        } else {
            theme::empty_card(
                ui,
                icon::SQUARE_SPLIT_HORIZONTAL,
                "Layout is unavailable.",
                "Reload to request the desk layout.",
            );
        }
    }
}

fn update_edits(edits: &mut Vec<(String, String)>, key: &str, value: &str, original: &str) {
    edits.retain(|(edit, _)| edit != key);
    if value != original {
        edits.push((key.to_owned(), value.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_edits_reverted_value_removes_only_its_pending_change() {
        // Arrange
        let mut edits = vec![
            ("hotkeys.home".into(), "home".into()),
            ("peers.example.clipboard".into(), "ask".into()),
        ];

        // Act
        update_edits(&mut edits, "peers.example.clipboard", "both", "both");

        // Assert
        assert_eq!(edits, vec![("hotkeys.home".into(), "home".into())]);
    }
}
