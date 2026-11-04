// SPDX-License-Identifier: GPL-3.0-or-later

//! Durable validated desk layout; an unusable file never blocks startup.

use std::io;
use std::path::{Path, PathBuf};

use a7n_core::layout::Layout;
use a7n_platform::fs::write_user_only;
use a7n_proto::{MachineId, v1};
use prost::Message;

pub(crate) struct LayoutStore {
    path: PathBuf,
    pub(crate) state: Layout,
    pub(crate) lost_edit: bool,
}

impl LayoutStore {
    pub(crate) fn load(path: &Path, own_id: MachineId) -> Self {
        let state = match read_layout(path) {
            Ok(Some(state)) => state,
            Ok(None) => Layout::empty(own_id),
            Err(()) => {
                crate::log::event!("kind=layout_file_invalid");
                Layout::empty(own_id)
            }
        };
        Self {
            path: path.to_owned(),
            state,
            lost_edit: false,
        }
    }

    pub(crate) fn persist(&mut self, state: Layout) -> io::Result<()> {
        write_user_only(&self.path, &state.to_proto().encode_to_vec())?;
        self.state = state;
        Ok(())
    }
}

fn read_layout(path: &Path) -> Result<Option<Layout>, ()> {
    let Some(wire) = crate::control_file::read::<v1::Layout>(path)? else {
        return Ok(None);
    };
    Layout::try_from(&wire).map(Some).map_err(|_| ())
}
