// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use a7n_proto::limits;
use prost::Message;

/// Reads a stored control message, bounded like one on the wire.
/// A missing file is `None`; unreadable, oversized or undecodable files are errors.
pub(crate) fn read<M: Message + Default>(path: &Path) -> Result<Option<M>, ()> {
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
    M::decode(bytes.as_slice()).map(Some).map_err(|_| ())
}
