// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use a7n_ipc::config::Config;
use a7n_ipc::{Request, Response, StatusInfo};
use eframe::egui;

mod canvas;
mod screens;
mod theme;
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
    address: String,
    code: String,
    pairing: Option<(String, Instant)>,
    layout: Option<canvas::Canvas>,
    config: Option<Config>,
    edits: Vec<(String, String)>,
    #[cfg(feature = "tray")]
    quit: bool,
    #[cfg(feature = "tray")]
    tray: Option<a7n_platform::tray::Tray>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, std::io::Error> {
        theme::install(&cc.egui_ctx);
        let worker = Worker::new(cc.egui_ctx.clone())?;
        #[cfg(not(feature = "tray"))]
        let message = String::new();
        #[cfg(feature = "tray")]
        let (tray, message) = {
            let ctx = cc.egui_ctx.clone();
            match a7n_platform::tray::Tray::new(move || ctx.request_repaint()) {
                Ok(tray) => (Some(tray), String::new()),
                Err(error) => (None, error.to_string()),
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
            address: String::new(),
            code: String::new(),
            pairing: None,
            layout: None,
            config: None,
            edits: Vec::new(),
            #[cfg(feature = "tray")]
            quit: false,
            #[cfg(feature = "tray")]
            tray,
        };
        app.send(Command::Config);
        Ok(app)
    }

    fn send(&mut self, command: Command) {
        if let Err(error) = self.worker.commands.try_send(command) {
            self.message = format!("Could not queue action: {error}; try again");
        }
    }

    fn updates(&mut self) {
        while let Ok(update) = self.worker.updates.try_recv() {
            match update {
                Update::Response(Ok(Response::Status(status))) => {
                    let was_missing = self.missing_permissions().is_some();
                    if was_missing && status.missing_permissions.is_empty() {
                        self.message = "Permissions granted".into();
                    }
                    self.status = Some(status);
                    self.daemon_error = None;
                }
                Update::Response(Ok(Response::Layout(info))) => {
                    match canvas::Canvas::try_from(info) {
                        Ok(canvas) => self.layout = Some(canvas),
                        Err(error) => self.message = error.to_string(),
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
                    self.message = format!("Paired with {name}");
                    self.pairing = None;
                }
                Update::Response(Ok(Response::Done)) => self.message = "Done".into(),
                Update::Response(Ok(_)) => self.message = "Unexpected daemon response".into(),
                Update::Response(Err(error)) => {
                    self.status = None;
                    self.daemon_error = Some(error);
                }
                Update::Config(Ok(config)) => {
                    self.config = Some(*config);
                    self.edits.clear();
                    self.editing_hotkeys = [false; 4];
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
        egui::Window::new("Clipboard confirmation")
            .collapsible(false)
            .show(ctx, |ui| {
                theme::card(ui, |ui| {
                    ui.label(format!(
                        "Clipboard from {}: {} items, {} B, {}",
                        held.name,
                        held.items,
                        held.size,
                        held.kinds_label()
                    ));
                    ui.horizontal(|ui| {
                        if theme::button(
                            ui,
                            theme::ButtonKind::Primary,
                            theme::ButtonSize::Md,
                            None,
                            "Accept",
                        )
                        .clicked()
                        {
                            action = Some(Request::ClipboardAccept);
                        }
                        if theme::button(
                            ui,
                            theme::ButtonKind::Ghost,
                            theme::ButtonSize::Md,
                            None,
                            "Reject",
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
        let (status, focused, paused) =
            self.status
                .as_ref()
                .map_or(("Disconnected", "unknown".to_owned(), false), |s| {
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
                            "Input unavailable"
                        } else {
                            "Connected"
                        },
                        focused,
                        s.paused,
                    )
                });
        let waiting = self
            .status
            .as_ref()
            .and_then(|s| s.held_clipboard.as_ref())
            .map(|held| format!("Clipboard waiting from {}", held.name));
        tray.update(waiting.as_deref().unwrap_or(status), &focused, paused);
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
        self.updates();
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
                    (Screen::Machines, theme::icon::MONITOR, "Machines"),
                    (
                        Screen::Layout,
                        theme::icon::SQUARE_SPLIT_HORIZONTAL,
                        "Layout",
                    ),
                    (Screen::Hotkeys, theme::icon::KEYBOARD, "Hotkeys"),
                    (
                        Screen::Policies,
                        theme::icon::SHIELD_CHECK,
                        "Peers & policies",
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
                            (theme::BadgeKind::Danger, "Daemon stopped"),
                            |status| {
                                if status.paused {
                                    (theme::BadgeKind::Warning, "Paused")
                                } else {
                                    (theme::BadgeKind::Online, "Daemon running")
                                }
                            },
                        );
                        theme::badge(ui, kind, label);
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
                ui.set_max_width(theme::size::CONTENT_MAX_W);
                ui.add_enabled_ui(self.worker.commands.capacity() > 0, |ui| {
                    if self.status.is_none() {
                        self.daemon_unavailable(ui);
                        return;
                    }
                    let missing = self.missing_permissions();
                    egui::ScrollArea::vertical().show(ui, |ui| {
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

fn main() -> eframe::Result {
    eframe::run_native(
        "Apeirogon",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 650.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)?))),
    )
}
