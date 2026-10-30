// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use a7n_ipc::{DEFAULT_UDP_PORT, MachineId, PairingCode, PeerInfo, Request, StatusInfo};
use eframe::egui;

use crate::{App, Command, theme};
use theme::{BadgeKind, ButtonKind, ButtonSize, IconButtonKind, icon};

impl App {
    pub(crate) fn machines(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.status.clone() else {
            return;
        };
        theme::page_header(
            ui,
            "Machines",
            "Computers sharing this keyboard and mouse.",
            |ui| {
                if theme::button(
                    ui,
                    ButtonKind::Secondary,
                    ButtonSize::Md,
                    Some(if status.paused {
                        icon::PLAY
                    } else {
                        icon::PAUSE
                    }),
                    if status.paused {
                        "Resume input sharing"
                    } else {
                        "Pause input sharing"
                    },
                )
                .clicked()
                {
                    self.send(Command::Request(Request::SetPaused {
                        paused: !status.paused,
                    }));
                }
            },
        );
        self.page_message(ui);
        theme::overline(ui, "This machine");
        theme::card(ui, |ui| {
            machine_identity(
                ui,
                &status.name,
                status.machine,
                BadgeKind::Accent,
                "This machine",
            );
            if let Some(reason) = &status.input {
                theme::error(ui, &format!("Input unavailable: {reason}"));
            }
        });
        ui.add_space(theme::size::SECTION_GAP);
        self.paired_machines(ui, &status);
        ui.add_space(theme::size::SECTION_GAP);
        self.nearby_machines(ui, &status);
        ui.add_space(theme::size::SECTION_GAP);
        self.pairing_ui(ui);
    }

    fn paired_machines(&mut self, ui: &mut egui::Ui, status: &StatusInfo) {
        let peers: Vec<_> = status
            .peers
            .iter()
            .filter(|peer| peer.id != status.machine)
            .collect();
        theme::overline(ui, &format!("Paired machines · {}", peers.len()));
        if peers.is_empty() {
            theme::empty_card(
                ui,
                icon::MONITOR,
                "No paired machines yet.",
                "Use Host or Join below to pair a machine.",
            );
        }
        for peer in peers {
            self.paired_machine(ui, status, peer);
        }
    }

    fn paired_machine(&mut self, ui: &mut egui::Ui, status: &StatusInfo, peer: &PeerInfo) {
        theme::card(ui, |ui| {
            machine_identity(
                ui,
                &peer.name,
                peer.id,
                if peer.connected {
                    BadgeKind::Online
                } else {
                    BadgeKind::Offline
                },
                if peer.connected { "Online" } else { "Offline" },
            );
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::button(
                        ui,
                        ButtonKind::DangerQuiet,
                        ButtonSize::Md,
                        Some(icon::X_CIRCLE),
                        "Unpair",
                    )
                    .clicked()
                    {
                        self.send(Command::Request(Request::Unpair { machine: peer.id }));
                    }
                    let nearby = status.nearby.iter().find(|nearby| nearby.id == peer.id);
                    ui.add_enabled_ui(!peer.connected && nearby.is_some(), |ui| {
                        if theme::button(ui, ButtonKind::Secondary, ButtonSize::Md, None, "Connect")
                            .on_hover_text(
                                "Connect using the discovered address, or enter an address below.",
                            )
                            .clicked()
                            && let Some(nearby) = nearby
                        {
                            self.send(Command::Request(Request::Connect { addr: nearby.addr }));
                        }
                    });
                });
            });
        });
    }

    fn nearby_machines(&mut self, ui: &mut egui::Ui, status: &StatusInfo) {
        theme::overline(ui, "Nearby machines");
        let nearby: Vec<_> = status
            .nearby
            .iter()
            .filter(|nearby| {
                nearby.id != status.machine && !status.peers.iter().any(|peer| peer.id == nearby.id)
            })
            .collect();
        if nearby.is_empty() {
            theme::empty_card(
                ui,
                icon::BROADCAST,
                "No machines found on your network yet.",
                "Apeirogon listens for other machines running the daemon on the same subnet.",
            );
        }
        for nearby in nearby {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::icon_tile(ui, icon::MONITOR);
                    ui.vertical(|ui| {
                        ui.label("Nearby machine");
                        machine_id(ui, nearby.id);
                        ui.label(
                            egui::RichText::new(nearby.addr.to_string())
                                .font(theme::font_mono(13.0)),
                        );
                    });
                });
                if theme::button(
                    ui,
                    ButtonKind::Secondary,
                    ButtonSize::Md,
                    None,
                    "Pair with this machine",
                )
                .clicked()
                {
                    self.address = nearby.addr.to_string();
                }
            });
        }
    }

    fn pairing_ui(&mut self, ui: &mut egui::Ui) {
        theme::overline(ui, "Pair a machine");
        ui.columns(2, |columns| {
            theme::card(&mut columns[0], |ui| {
                ui.heading("Host pairing");
                ui.label("Show a one-time code on this machine and enter it on the other one.");
                if let Some((code, expiry)) = &self.pairing {
                    let remaining = expiry.saturating_duration_since(Instant::now()).as_secs();
                    if remaining == 0 {
                        theme::small(ui, "Pairing code expired");
                    } else {
                        ui.label(egui::RichText::new(code).font(theme::font_mono(13.0)));
                        theme::small(ui, &format!("Expires in {remaining} s"));
                    }
                }
                if theme::button(
                    ui,
                    ButtonKind::Primary,
                    ButtonSize::Md,
                    Some(icon::BROADCAST),
                    "Show pairing code",
                )
                .clicked()
                {
                    self.send(Command::Request(Request::PairHost));
                }
            });
            theme::card(&mut columns[1], |ui| self.join_pairing(ui));
        });
    }

    fn join_pairing(&mut self, ui: &mut egui::Ui) {
        ui.heading("Join pairing");
        ui.label("Enter the host's address and the code it shows.");
        theme::text_input(ui, "Address", &mut self.address, "IP or IP:port", true);
        theme::text_input(ui, "Pairing code", &mut self.code, "Six digits", true);
        let address = parse_address(&self.address);
        let code = PairingCode::parse(self.code.trim());
        if !self.address.is_empty() && address.is_err() {
            theme::error(ui, "Enter an IP or IP:port; use [IPv6]:port");
        }
        if !self.code.is_empty() && code.is_err() {
            theme::error(ui, "Code must be six ASCII digits");
        }
        ui.add_enabled_ui(address.is_ok() && code.is_ok(), |ui| {
            if theme::button(
                ui,
                ButtonKind::Secondary,
                ButtonSize::Md,
                None,
                "Join pairing",
            )
            .clicked()
                && let (Ok(addr), Ok(code)) = (address, code)
            {
                self.send(Command::Request(Request::PairJoin { addr, code }));
                self.code.clear();
            }
        });
        ui.add_enabled_ui(address.is_ok(), |ui| {
            if theme::button(
                ui,
                ButtonKind::Ghost,
                ButtonSize::Sm,
                None,
                "Connect to paired address",
            )
            .clicked()
                && let Ok(addr) = address
            {
                self.send(Command::Request(Request::Connect { addr }));
            }
        });
    }
}

fn machine_identity(ui: &mut egui::Ui, name: &str, id: MachineId, kind: BadgeKind, status: &str) {
    ui.horizontal(|ui| {
        theme::icon_tile(ui, icon::MONITOR);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.spacing_mut().interact_size.y = 22.0;
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(name)
                        .font(theme::font_medium(14.0))
                        .color(theme::Palette::of(ui.ctx()).text),
                );
                theme::badge(ui, kind, status);
            });
            id_label(ui, id);
        });
        copy_id(ui, id);
    });
}

fn machine_id(ui: &mut egui::Ui, id: MachineId) {
    ui.horizontal(|ui| {
        id_label(ui, id);
        copy_id(ui, id);
    });
}

fn id_label(ui: &mut egui::Ui, id: MachineId) {
    ui.label(
        egui::RichText::new(id.to_string())
            .font(theme::font_mono(12.0))
            .color(theme::Palette::of(ui.ctx()).text3),
    );
}

fn copy_id(ui: &mut egui::Ui, id: MachineId) {
    if theme::icon_button(
        ui,
        IconButtonKind::Ghost,
        icon::COPY,
        "Copy machine ID",
        false,
    )
    .clicked()
    {
        ui.ctx().copy_text(id.to_string());
    }
}

fn parse_address(text: &str) -> Result<SocketAddr, ()> {
    text.trim()
        .parse()
        .or_else(|_| {
            text.trim()
                .parse::<IpAddr>()
                .map(|ip| SocketAddr::new(ip, DEFAULT_UDP_PORT))
        })
        .map_err(|_| ())
}
