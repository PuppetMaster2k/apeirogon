// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::FirewallProblem;
use eframe::egui;

use crate::theme;

const FIX_FONT_SIZE: f32 = 12.0;

/// One card per problem, with the fix as selectable text to copy.
pub(super) fn firewall_warnings(ui: &mut egui::Ui, problems: &[FirewallProblem]) {
    for problem in problems {
        theme::card(ui, |ui| {
            theme::warning(ui, &format!("Windows Firewall: {problem}"));
            ui.add_space(theme::size::ROW_GAP / 2.0);
            theme::small(
                ui,
                "How to fix it. Commands need an administrator Command Prompt.",
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(problem.fix())
                        .font(theme::font_mono(FIX_FONT_SIZE))
                        .color(theme::Palette::of(ui.ctx()).text),
                )
                .selectable(true),
            );
        });
        ui.add_space(theme::size::ROW_GAP);
    }
}
