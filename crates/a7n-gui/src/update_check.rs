// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use eframe::egui;
use rust_i18n::t;
use serde::Deserialize;

use crate::{App, Command, theme};

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
const CHECK_INTERVAL: Duration = Duration::from_secs(SECONDS_PER_DAY);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const RELEASE_API: &str = "https://api.github.com/repos/PuppetMaster2k/apeirogon/releases/latest";

#[derive(Default)]
pub(super) struct State {
    last_attempt: Option<Instant>,
    manual: Option<bool>,
    release: Option<Release>,
    message: String,
}

#[derive(Deserialize)]
pub(super) struct Release {
    tag_name: String,
    html_url: String,
}

pub(super) fn check() -> Result<Option<Release>, String> {
    let fetch = || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .https_only(true)
            .max_redirects(0)
            .user_agent("apeirogon-gui")
            .build()
            .into();
        let body = agent.get(RELEASE_API).call()?.body_mut().read_to_string()?;
        let release: Release = serde_json::from_str(&body)?;
        let latest = version(&release.tag_name).ok_or("Invalid release version")?;
        let current = version(env!("CARGO_PKG_VERSION")).ok_or("Invalid GUI version")?;
        if !release
            .html_url
            .starts_with("https://github.com/PuppetMaster2k/apeirogon/releases/")
        {
            return Err("Invalid release URL".into());
        }
        Ok::<_, Box<dyn std::error::Error>>((latest > current).then_some(release))
    };
    fetch().map_err(|_| t!("Update check failed; try later.").into())
}

fn version(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let mut parts = value.split('-').next()?.split('.');
    let result = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(result)
}

impl App {
    pub(super) fn check_updates(&mut self, manual: bool) {
        if self.update_check.manual.is_some() {
            return;
        }
        if !manual
            && (!self.config.as_ref().is_some_and(|c| c.gui.update_check)
                || self
                    .update_check
                    .last_attempt
                    .is_some_and(|last| last.elapsed() < CHECK_INTERVAL))
        {
            return;
        }
        if self.worker.commands.try_send(Command::CheckUpdates).is_ok() {
            self.update_check.last_attempt = Some(Instant::now());
            self.update_check.manual = Some(manual);
            self.update_check.message.clear();
        } else if manual {
            self.update_check.message = t!("Update check busy; try again.").into();
        }
    }

    pub(super) fn update_checked(&mut self, result: Result<Option<Release>, String>) {
        self.update_check.last_attempt = Some(Instant::now());
        let manual = self.update_check.manual.take() == Some(true);
        match result {
            Ok(release) => {
                if manual && release.is_none() {
                    self.update_check.message = t!("Up to date").into();
                }
                self.update_check.release = release;
            }
            Err(error) if manual => self.update_check.message = error,
            Err(_) => {}
        }
    }

    pub(super) fn update_notice(&self, ui: &mut egui::Ui) {
        if self.update_check.release.is_none() && self.update_check.message.is_empty() {
            return;
        }
        theme::card(ui, |ui| {
            if let Some(release) = &self.update_check.release {
                ui.hyperlink_to(
                    t!(
                        "Apeirogon %{version} is available",
                        version = release.tag_name
                    ),
                    &release.html_url,
                );
            }
            if !self.update_check.message.is_empty() {
                theme::small(ui, &self.update_check.message);
            }
        });
        ui.add_space(theme::size::SECTION_GAP);
    }

    pub(super) fn update_options(&mut self, ui: &mut egui::Ui) {
        const KEY: &str = "gui.update_check";
        let mut enabled = self.value(KEY, "false") == "true";
        theme::card(ui, |ui| {
            theme::setting_row(
                ui,
                &t!("Updates"),
                &t!("Check GitHub daily; no machine data, downloads or installs."),
                Some(KEY),
                |ui| {
                    if theme::toggle(ui, &mut enabled).changed() {
                        self.edit(KEY, if enabled { "true" } else { "false" }, "false");
                    }
                    ui.add_enabled_ui(self.update_check.manual.is_none(), |ui| {
                        if theme::button(
                            ui,
                            theme::ButtonKind::Ghost,
                            theme::ButtonSize::Sm,
                            None,
                            &t!("Check"),
                        )
                        .clicked()
                        {
                            self.check_updates(true);
                        }
                    });
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_release_tags_compare_by_numeric_components() {
        // Arrange
        let current = version("1.2.3");
        let cases = [
            ("1.2.4", std::cmp::Ordering::Greater),
            ("1.2.3", std::cmp::Ordering::Equal),
            ("1.2.2", std::cmp::Ordering::Less),
            ("v1.10.0", std::cmp::Ordering::Greater),
            ("v1.2.3-preview", std::cmp::Ordering::Equal),
        ];

        for (tag, expected) in cases {
            // Act
            let ordering = version(tag).cmp(&current);

            // Assert
            assert_eq!(ordering, expected, "{tag}");
        }
    }
}
