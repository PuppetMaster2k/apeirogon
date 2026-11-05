// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{
    FileTransfer, HeldFiles, MachineId, Request, StatusInfo, TransferDirection, format_size,
};
use eframe::egui;

use crate::{App, Command, theme};
use theme::{ButtonKind, ButtonSize, icon};

impl App {
    pub(super) fn held_files(&mut self, ui: &mut egui::Ui, status: &StatusInfo) {
        if status.held_files.is_empty() {
            return;
        }
        theme::overline(ui, "File offers");
        for held in &status.held_files {
            self.held_offer(ui, status, held);
        }
        ui.add_space(theme::size::SECTION_GAP);
    }

    fn held_offer(&mut self, ui: &mut egui::Ui, status: &StatusInfo, held: &HeldFiles) {
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                theme::icon_tile(ui, icon::MONITOR);
                ui.add(
                    egui::Label::new(format!(
                        "{} wants to send {} files ({})",
                        peer_name(status, held.peer),
                        held.files,
                        format_size(held.total_size),
                    ))
                    .wrap(),
                );
            });
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::button(ui, ButtonKind::Primary, ButtonSize::Md, None, "Accept")
                        .clicked()
                    {
                        self.send(Command::Request(Request::FilesAccept {
                            peer: held.peer,
                            offer_id: held.offer_id,
                        }));
                    }
                    if theme::button(ui, ButtonKind::Ghost, ButtonSize::Md, None, "Reject")
                        .clicked()
                    {
                        self.send(Command::Request(Request::FilesReject { peer: held.peer }));
                    }
                });
            });
        });
    }

    pub(super) fn active_transfers(&mut self, ui: &mut egui::Ui, status: &StatusInfo) {
        if status.transfers.is_empty() {
            return;
        }
        theme::overline(ui, "File transfers");
        for transfer in &status.transfers {
            self.transfer_card(ui, status, transfer);
        }
        ui.add_space(theme::size::SECTION_GAP);
    }

    fn transfer_card(&mut self, ui: &mut egui::Ui, status: &StatusInfo, transfer: &FileTransfer) {
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                let (verb, preposition) = match transfer.direction {
                    TransferDirection::Send => ("Sending", "to"),
                    TransferDirection::Receive => ("Receiving", "from"),
                };
                ui.add(
                    egui::Label::new(format!(
                        "{verb} {preposition} {}",
                        peer_name(status, transfer.peer)
                    ))
                    .truncate(),
                );
            });
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::button(
                        ui,
                        ButtonKind::DangerQuiet,
                        ButtonSize::Md,
                        Some(icon::X_CIRCLE),
                        "Cancel",
                    )
                    .clicked()
                    {
                        self.send(Command::Request(Request::CancelTransfer {
                            id: transfer.id,
                        }));
                    }
                });
            });
            ui.add(egui::ProgressBar::new(fraction(transfer)));
            theme::small(
                ui,
                &format!(
                    "{}/{} files, {} / {}",
                    transfer.files_done,
                    transfer.files_total,
                    format_size(transfer.bytes_done),
                    format_size(transfer.bytes_total),
                ),
            );
        });
    }
}

fn peer_name(status: &StatusInfo, id: MachineId) -> String {
    status
        .peers
        .iter()
        .find(|peer| peer.id == id)
        .map_or_else(|| id.to_string(), |peer| peer.name.clone())
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a progress bar needs only display precision"
)]
fn fraction(transfer: &FileTransfer) -> f32 {
    if transfer.bytes_total == 0 {
        return 0.0;
    }
    (transfer.bytes_done as f64 / transfer.bytes_total as f64).clamp(0.0, 1.0) as f32
}
