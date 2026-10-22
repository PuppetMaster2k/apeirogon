// SPDX-License-Identifier: GPL-3.0-or-later

//! Durable validated desk layout; an unusable file never blocks startup.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use a7n_core::layout::Layout;
use a7n_platform::fs::write_user_only;
use a7n_proto::{MachineId, limits, v1};
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
                eprintln!("kind=layout_file_invalid");
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
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut bytes = Vec::new();
    file.take((limits::CONTROL_FRAME_MAX + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > limits::CONTROL_FRAME_MAX {
        return Err(());
    }
    let wire = v1::Layout::decode(bytes.as_slice()).map_err(|_| ())?;
    Layout::try_from(&wire).map(Some).map_err(|_| ())
}
