// SPDX-License-Identifier: GPL-3.0-or-later

//! Durable validated settings snapshot; an unusable file never blocks startup.

use std::io;
use std::path::{Path, PathBuf};

use a7n_core::settings::Settings;
use a7n_platform::fs::write_user_only;
use a7n_proto::v1;
use prost::Message;

pub(crate) struct SettingsStore {
    path: PathBuf,
    pub(crate) state: Option<Settings>,
}

impl SettingsStore {
    pub(crate) fn load(path: &Path) -> Self {
        let state = read_settings(path).unwrap_or_else(|()| {
            crate::log::event!("kind=settings_file_invalid");
            None
        });
        Self {
            path: path.to_owned(),
            state,
        }
    }

    pub(crate) fn persist(&mut self, state: Settings) -> io::Result<()> {
        write_user_only(&self.path, &state.to_proto().encode_to_vec())?;
        self.state = Some(state);
        Ok(())
    }
}

fn read_settings(path: &Path) -> Result<Option<Settings>, ()> {
    let Some(wire) = crate::control_file::read::<v1::SettingsSync>(path)? else {
        return Ok(None);
    };
    Settings::try_from(&wire).map(Some).map_err(|_| ())
}
