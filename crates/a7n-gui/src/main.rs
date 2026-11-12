// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use a7n_ipc::config::{Config, Language};
use a7n_ipc::{Request, Response, StatusInfo};
use eframe::egui;
use rust_i18n::t;

rust_i18n::i18n!("locales", fallback = "en");

mod canvas;
mod screens;
mod theme;
mod update_check;
mod worker;

use worker::{Command, Update, Worker};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Machines,
    Layout,
    Hotkeys,
    Policies,
}

struct App {
    worker: Worker,
    screen: Screen,
    status: Option<StatusInfo>,
    message: String,
    daemon_error: Option<String>,
    selected_peer: Option<a7n_ipc::MachineId>,
    editing_hotkeys: [bool; 4],
    editing_gestures: [bool; 4],
    recording_hotkey: Option<String>,
    editing_receive_dir: bool,
    address: String,
    code: String,
    pairing: Option<(String, Instant)>,
    layout: Option<canvas::Canvas>,
    config: Option<Config>,
    edits: Vec<(String, String)>,
    update_check: update_check::State,
    #[cfg(feature = "tray")]
    quit: bool,
    #[cfg(feature = "tray")]
    tray: Option<a7n_platform::tray::Tray>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, std::io::Error> {
        set_language(Language::System);
        theme::install(&cc.egui_ctx);
        let worker = Worker::new(cc.egui_ctx.clone())?;
        #[cfg(not(feature = "tray"))]
        let message = String::new();
        #[cfg(feature = "tray")]
        let (tray, message) = {
            let ctx = cc.egui_ctx.clone();
            match a7n_platform::tray::Tray::new(move || ctx.request_repaint()) {
                Ok(tray) => (Some(tray), String::new()),
                Err(error) => (
                    None,
                    t!("Could not create tray: %{error}", error = error).into_owned(),
                ),
            }
        };
        let mut app = Self {
            worker,
            screen: Screen::Machines,
            status: None,
            message,
            daemon_error: None,
            selected_peer: None,
            editing_hotkeys: [false; 4],
            editing_gestures: [false; 4],
            recording_hotkey: None,
            editing_receive_dir: false,
            address: String::new(),
            code: String::new(),
            pairing: None,
            layout: None,
            config: None,
            edits: Vec::new(),
            update_check: update_check::State::default(),
            #[cfg(feature = "tray")]
            quit: false,
            #[cfg(feature = "tray")]
            tray,
        };
        #[cfg(feature = "tray")]
        app.tray(&cc.egui_ctx);
        app.send(Command::Config);
        Ok(app)
    }

    fn send(&mut self, command: Command) {
        if let Err(error) = self.worker.commands.try_send(command) {
            self.message =
                t!("Could not queue action: %{error}; try again", error = error).into_owned();
            if matches!(
                error.into_inner(),
                Command::Request(Request::RecordHotkey { .. })
            ) {
                self.recording_hotkey = None;
            }
        }
    }

    fn updates(&mut self, ctx: &egui::Context) {
        while let Ok(update) = self.worker.updates.try_recv() {
            match update {
                Update::Release(result) => self.update_checked(result),
                Update::Recorded(result) => {
                    if let Some(key) = self.recording_hotkey.take() {
                        match result {
                            Ok(mut value) => {
                                if key == "input.local_keys" {
                                    let current = self.value(&key, "");
                                    if current.split(',').any(|name| name == value) {
                                        continue;
                                    }
                                    if !current.is_empty() {
                                        value = format!("{current},{value}");
                                    }
                                }
                                self.edit(&key, &value, "");
                            }
                            Err(message) => self.message = message,
                        }
                    }
                }
                Update::Response(Ok(Response::Status(status))) => {
                    let was_missing = self.missing_permissions().is_some();
                    if was_missing && status.missing_permissions.is_empty() {
                        self.message = t!("Permissions granted").into_owned();
                    }
                    self.status = Some(status);
                    self.daemon_error = None;
                }
                Update::Response(Ok(Response::Layout(info))) => {
                    match canvas::Canvas::try_from(info) {
                        Ok(canvas) => self.layout = Some(canvas),
                        Err(_) => {
                            self.message = t!("No canvas space to place machines").into_owned();
                        }
                    }
                }
                Update::Response(Ok(Response::PairCode {
                    code,
                    expires_in_secs,
                })) => {
                    self.pairing = Instant::now()
                        .checked_add(Duration::from_secs(expires_in_secs))
                        .map(|expiry| (code.to_string(), expiry));
                }
                Update::Response(Ok(Response::Error { message, .. })) | Update::Notice(message) => {
                    self.message = message;
                }
                Update::Response(Ok(Response::Paired { name, .. })) => {
                    self.message = t!("Paired with %{name}", name = name).into_owned();
                    self.pairing = None;
                }
                Update::Response(Ok(Response::Waking { name })) => {
                    self.message = t!("Waking %{name}…", name = name).into_owned();
                }
                Update::Response(Ok(Response::Done)) => self.message = t!("Done").into_owned(),
                Update::Response(Ok(_)) => {
                    self.message = t!("Unexpected daemon response").into_owned();
                }
                Update::Response(Err(error)) => {
                    self.status = None;
                    self.daemon_error = Some(error);
                }
                Update::Config(Ok(config)) => {
                    set_language(config.gui.language);
                    self.config = Some(*config);
                    self.edits.clear();
                    self.apply_appearance(ctx);
                    self.editing_hotkeys = [false; 4];
                    self.editing_gestures = [false; 4];
                    self.editing_receive_dir = false;
                }
                Update::Config(Err(error)) => {
                    self.config = None;
                    self.message = error;
                }
            }
        }
    }

    /// Permissions the daemon still needs, if any; this swaps in the onboarding screen.
    fn missing_permissions(&self) -> Option<Vec<a7n_ipc::Permission>> {
        let missing = &self.status.as_ref()?.missing_permissions;
        (!missing.is_empty()).then(|| missing.clone())
    }

    fn clipboard_prompt(&mut self, ctx: &egui::Context) {
        let Some(held) = self.status.as_ref().and_then(|s| s.held_clipboard.as_ref()) else {
            return;
        };
        let mut action = None;
        egui::Window::new(t!("Clipboard confirmation"))
            .collapsible(false)
            .show(ctx, |ui| {
                theme::card(ui, |ui| {
                    let kinds = held
                        .kinds
                        .iter()
                        .map(|kind| match kind {
                            a7n_ipc::ClipboardKind::Text => t!("Text"),
                            a7n_ipc::ClipboardKind::Image => t!("Image"),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    ui.label(t!(
                        "Clipboard from %{name}: %{items} items, %{size} B, %{kinds}",
                        name = held.name,
                        items = held.items,
                        size = held.size,
                        kinds = kinds
                    ));
                    ui.horizontal(|ui| {
                        if theme::button(
                            ui,
                            theme::ButtonKind::Primary,
                            theme::ButtonSize::Md,
                            None,
                            &t!("Accept"),
                        )
                        .clicked()
                        {
                            action = Some(Request::ClipboardAccept {
                                machine: held.machine,
                                generation: held.generation,
                            });
                        }
                        if theme::button(
                            ui,
                            theme::ButtonKind::Ghost,
                            theme::ButtonSize::Md,
                            None,
                            &t!("Reject"),
                        )
                        .clicked()
                        {
                            action = Some(Request::ClipboardReject);
                        }
                    });
                });
            });
        if let Some(request) = action {
            self.send(Command::Request(request));
            self.send(Command::Request(Request::Status));
        }
    }

    #[cfg(feature = "tray")]
    fn tray(&mut self, ctx: &egui::Context) {
        use a7n_platform::tray::Action;
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

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.updates(ctx);
        self.check_updates(false);
        #[cfg(feature = "tray")]
        self.tray(ctx);
        // Keeps the pairing expiry and native tray responsive while idle.
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.clipboard_prompt(ui.ctx());
        let p = theme::Palette::of(ui.ctx());
        egui::Panel::left("navigation")
            .exact_size(theme::size::SIDEBAR_W)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(p.sidebar)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .inner_margin(egui::Margin::symmetric(12, 16)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Apeirogon").font(theme::font_semibold(15.0)));
                    ui.label(
                        egui::RichText::new("a7n")
                            .font(theme::font_mono(11.0))
                            .color(p.text3),
                    );
                });
                ui.add_space(20.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                for (screen, icon, name) in [
                    (Screen::Machines, theme::icon::MONITOR, &t!("Machines")),
                    (
                        Screen::Layout,
                        theme::icon::SQUARE_SPLIT_HORIZONTAL,
                        &t!("Layout"),
                    ),
                    (Screen::Hotkeys, theme::icon::KEYBOARD, &t!("Hotkeys")),
                    (
                        Screen::Policies,
                        theme::icon::SHIELD_CHECK,
                        &t!("Peers & policies"),
                    ),
                ] {
                    if theme::nav_item(ui, icon, name, self.screen == screen).clicked() {
                        self.screen = screen;
                        if screen == Screen::Layout {
                            self.send(Command::Request(Request::GetLayout));
                        }
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    theme::card(ui, |ui| {
                        let (kind, label) = self.status.as_ref().map_or(
                            (theme::BadgeKind::Danger, t!("Daemon stopped")),
                            |status| {
                                if status.paused {
                                    (theme::BadgeKind::Warning, t!("Paused"))
                                } else {
                                    (theme::BadgeKind::Online, t!("Daemon running"))
                                }
                            },
                        );
                        theme::badge(ui, kind, &label);
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(40, 32)),
            )
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(theme::size::CONTENT_MAX_W));
                ui.add_enabled_ui(self.worker.commands.capacity() > 0, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.update_notice(ui);
                            if self.status.is_none() {
                                self.daemon_unavailable(ui);
                                return;
                            }
                            let missing = self.missing_permissions();
                            if let Some(missing) = &missing {
                                self.permissions(ui, missing);
                                return;
                            }
                            match self.screen {
                                Screen::Machines => self.machines(ui),
                                Screen::Layout => self.layout_screen(ui),
                                Screen::Hotkeys | Screen::Policies => self.settings(ui),
                            }
                        });
                });
            });
    }
}

fn set_language(language: Language) {
    static SYSTEM_LOCALE: OnceLock<&str> = OnceLock::new();
    let locale = match language {
        Language::System => SYSTEM_LOCALE.get_or_init(|| {
            if sys_locale::get_locale()
                .is_some_and(|locale| locale.to_ascii_lowercase().starts_with("de"))
            {
                "de"
            } else {
                "en"
            }
        }),
        Language::English => "en",
        Language::German => "de",
    };
    rust_i18n::set_locale(locale);
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Apeirogon",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([900.0, 650.0])
                .with_min_inner_size([720.0, 480.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)?))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_german_locale_translates_known_key_and_preserves_unknown() {
        // Arrange
        rust_i18n::set_locale("de");

        // Act
        let translated = t!("Machines");
        let fallback = t!("Unknown translation key");

        // Assert
        assert_eq!(translated, "Rechner");
        assert_eq!(fallback, "Unknown translation key");
        rust_i18n::set_locale("en");
    }
}
