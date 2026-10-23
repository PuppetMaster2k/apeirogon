// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{self, Write};

use a7n_ipc::ClipboardInfo;
use a7n_ipc::config::ClipboardPolicy;

const KIBIBYTE: u64 = 1024;

pub(super) fn write_status(output: &mut impl Write, info: &ClipboardInfo) -> io::Result<()> {
    let availability = if info.available {
        "available"
    } else {
        "unavailable (no backend for this session)"
    };
    writeln!(output, "clipboard: {availability}")?;
    if let Some(copy) = &info.current {
        let origin = copy
            .origin_name
            .clone()
            .unwrap_or_else(|| copy.origin.to_string());
        write!(
            output,
            "current copy: from {origin} (generation {}), {} items, ",
            copy.generation, copy.items
        )?;
        if copy.size < KIBIBYTE {
            writeln!(output, "{} B", copy.size)?;
        } else {
            writeln!(
                output,
                "{}.{} KiB",
                copy.size / KIBIBYTE,
                (copy.size % KIBIBYTE) * 10 / KIBIBYTE
            )?;
        }
    } else {
        writeln!(output, "current copy: none")?;
    }
    write!(output, "peers:")?;
    if info.peers.is_empty() {
        write!(output, " none")?;
    }
    for (index, peer) in info.peers.iter().enumerate() {
        let separator = if index == 0 { "" } else { "," };
        let policy = match peer.policy {
            ClipboardPolicy::Both => "both",
            ClipboardPolicy::Send => "send",
            ClipboardPolicy::Receive => "receive",
            ClipboardPolicy::Off => "off",
        };
        let link = if peer.linked { "linked" } else { "not linked" };
        write!(output, "{separator} {} {policy} {link}", peer.name)?;
    }
    writeln!(output)
}
