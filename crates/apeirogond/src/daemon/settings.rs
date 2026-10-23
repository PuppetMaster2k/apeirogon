// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{SystemTime, UNIX_EPOCH};

use a7n_core::settings::{SYNCED_KEYS, Settings, newer};
use a7n_proto::v1::{Envelope, SettingsSync, envelope::Body};

use super::{Daemon, DaemonError};

impl Daemon {
    pub(super) fn settings_message(&self) -> Option<Envelope> {
        if !self.config.sync.settings {
            return None;
        }
        self.settings.state.as_ref().map(|state| Envelope {
            body: Some(Body::Settings(state.to_proto())),
        })
    }

    pub(super) fn detect_settings_edit(&mut self) -> Result<(), DaemonError> {
        let values = SYNCED_KEYS
            .iter()
            .map(|key| {
                (
                    (*key).to_owned(),
                    self.config
                        .get(key)
                        .expect("sync keys are supported config scalars"),
                )
            })
            .collect();
        if self
            .settings
            .state
            .as_ref()
            .is_some_and(|state| state.values == values)
        {
            return Ok(());
        }
        let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
            .map_err(std::io::Error::other)?;
        let after_previous = self
            .settings
            .state
            .as_ref()
            .map_or(0, |state| state.set_at.saturating_add(1));
        self.settings.persist(Settings {
            set_at: now.max(after_previous),
            by: self.identity.machine_id,
            values,
        })?;
        self.broadcast_settings();
        Ok(())
    }

    pub(super) fn broadcast_settings(&mut self) {
        if let Some(envelope) = self.settings_message() {
            self.broadcast(&envelope);
        }
    }

    pub(super) async fn receive_settings(
        &mut self,
        wire: &SettingsSync,
    ) -> Result<(), DaemonError> {
        if !self.config.sync.settings || !wire.on {
            return Ok(());
        }
        let Ok(incoming) = Settings::try_from(wire) else {
            eprintln!("kind=settings_rejected");
            return Ok(());
        };
        if !newer(self.settings.state.as_ref(), &incoming) {
            return Ok(());
        }
        let mut candidate = self.config.clone();
        if candidate
            .set_many(
                incoming
                    .values
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .is_err()
        {
            eprintln!("kind=settings_rejected");
            return Ok(());
        }
        candidate.save(&self.config_path)?;
        self.apply_config(candidate).await;
        self.settings.persist(incoming)?;
        self.broadcast_settings();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
