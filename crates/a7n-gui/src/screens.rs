// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use rust_i18n::t;

use super::{App, Command, Screen, theme};
use theme::{BadgeKind, ButtonKind, ButtonSize, IconButtonKind, icon};

mod chord_editor;
mod files;
mod firewall;
mod gestures;
mod hotkeys;
mod machines;
mod options;
mod peers;
mod permissions;

impl App {
    fn hide_addresses(&self) -> bool {
        self.value("gui.hide_addresses", "false") == "true"
    }

    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        self.page_message(ui);
        if self.config.is_none() {
            theme::empty_card(
                ui,
                icon::ARROW_CLOCKWISE,
                &t!("Config is unavailable."),
                &t!("Reload config to try again."),
            );
            return;
        }
        match self.screen {
            Screen::Hotkeys => {
                theme::overline(ui, &t!("Hotkeys"));
                theme::accordion_group(ui, |ui| {
                    self.hotkeys(ui);
                    self.general_options(ui);
                    self.input_options(ui);
                    if a7n_platform::input::GESTURE_CHORDS_SUPPORTED {
                        self.gestures(ui);
                    }
                });
            }
            Screen::Feedback => {
                self.appearance_options(ui);
                ui.add_space(theme::size::SECTION_GAP);
                self.feedback_options(ui);
            }
            _ => self.peers(ui),
        }
        if let Some(error) = self.config_validation_error() {
            theme::error(ui, &error);
        }
        ui.add_space(theme::size::SECTION_GAP);
        theme::small(
            ui,
            &t!("Saving rewrites config.toml without comments, like a7n config set."),
        );
    }

    pub(super) fn page_header(&mut self, ui: &mut egui::Ui) {
        if self.status.is_none() {
            theme::page_header(
                ui,
                &t!("Daemon isn't running"),
                &t!("Start the daemon to manage machines and share input."),
                |_| {},
            );
        } else if self.missing_permissions().is_some() {
            theme::page_header(
                ui,
                &t!("Allow Apeirogon to share your keyboard and mouse"),
                &t!("macOS asks you to approve each permission once."),
                |_| {},
            );
        } else {
            match self.screen {
                Screen::Machines => self.machines_header(ui),
                Screen::Layout => self.layout_header(ui),
                Screen::Hotkeys | Screen::Feedback | Screen::Policies => {
                    self.settings_page_header(ui);
                }
            }
        }
    }

    fn settings_page_header(&mut self, ui: &mut egui::Ui) {
        let (title, subtitle) = match self.screen {
            Screen::Hotkeys => (
                t!("Hotkeys"),
                t!("Shortcuts for returning home and moving input between machines."),
            ),
            Screen::Feedback => (
                t!("Feedback & theming"),
                t!("Choose visual feedback and the window theme."),
            ),
            _ => (
                t!("Peers & policies"),
                t!("What each paired machine is allowed to do."),
            ),
        };
        self.settings_header(ui, &title, &subtitle);
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
                        &t!("Save changes"),
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
                &t!("Discard edits and reload config"),
                false,
            )
            .clicked()
            {
                self.send(Command::Config);
            }
            if !self.edits.is_empty() {
                theme::badge(ui, BadgeKind::Warning, &t!("Unsaved changes"));
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
            .map(|error| t!("Invalid settings: %{error}", error = error).into_owned())
    }

    pub(crate) fn value(&self, key: &str, default: &str) -> String {
        self.edits
            .iter()
            .find(|(edit, _)| edit == key)
            .map(|(_, value)| value.clone())
            .or_else(|| self.config.as_ref().and_then(|config| config.get(key)))
            .unwrap_or_else(|| default.to_owned())
    }

    pub(super) fn edit(&mut self, key: &str, value: &str, default: &str) {
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
            theme::card(ui, |ui| {
                ui.vertical_centered(|ui| {
                    theme::icon_tile(ui, icon::POWER);
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
                        &t!("Start daemon"),
                    )
                    .clicked()
                    {
                        self.send(Command::Start);
                    }
                });
            });
        });
    }

    fn layout_header(&mut self, ui: &mut egui::Ui) {
        let mut save = false;
        theme::page_header(
            ui,
            &t!("Layout"),
            &t!("Arrange machines to match their positions on your desk."),
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
                            &t!("Save layout"),
                        )
                        .clicked();
                    },
                );
                if theme::button(
                    ui,
                    ButtonKind::Ghost,
                    ButtonSize::Md,
                    Some(icon::ARROW_CLOCKWISE),
                    &t!("Reload"),
                )
                .on_hover_text(t!("Discard edits and reload layout"))
                .clicked()
                {
                    self.send(Command::Request(a7n_ipc::Request::GetLayout));
                }
                if theme::button(
                    ui,
                    ButtonKind::Secondary,
                    ButtonSize::Md,
                    None,
                    &t!("Use real sizes"),
                )
                .clicked()
                    && let Some(canvas) = &mut self.layout
                    && let Err(_) = canvas.use_real_sizes()
                {
                    self.message = t!("No canvas space to place machines").into();
                }
                if theme::button(
                    ui,
                    ButtonKind::Secondary,
                    ButtonSize::Md,
                    None,
                    &t!("Fit to canvas"),
                )
                .clicked()
                    && let Some(canvas) = &mut self.layout
                {
                    canvas.fit();
                }
            },
        );
        if save
            && let Some(canvas) = &self.layout
            && canvas.is_valid()
        {
            let placed = canvas.placed();
            self.send(Command::Request(a7n_ipc::Request::SetLayout { placed }));
        }
    }

    pub(super) fn layout_screen(&mut self, ui: &mut egui::Ui) {
        self.page_message(ui);
        theme::overline(ui, &t!("Layout"));
        if let Some(canvas) = &mut self.layout {
            let p = theme::Palette::of(ui.ctx());
            ui.scope(|ui| {
                ui.visuals_mut().widgets.noninteractive.bg_fill = p.bg;
                theme::card(ui, |ui| canvas.show(ui));
            });
        } else {
            theme::empty_card(
                ui,
                icon::SQUARE_SPLIT_HORIZONTAL,
                &t!("Layout is unavailable."),
                &t!("Reload to request the desk layout."),
            );
        }
    }
}

fn address_text(value: &impl ToString, hidden: bool) -> String {
    if hidden {
        t!("Address hidden").into_owned()
    } else {
        value.to_string()
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
