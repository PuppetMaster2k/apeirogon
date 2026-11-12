// SPDX-License-Identifier: GPL-3.0-or-later

//! ARGB selection and bounded image uploads; no compositor means solid fallback.

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::render::{ConnectionExt as _, PictType};
use x11rb::protocol::xproto::{
    Colormap, ColormapAlloc, ConnectionExt as _, CreateGCAux, ImageFormat, ImageOrder, Visualid,
    Window,
};
use x11rb::rust_connection::RustConnection;

use super::super::raster::Frame;

pub(super) struct Argb {
    pub visual: Visualid,
    pub colormap: Colormap,
    gc: u32,
    little_endian: bool,
    upload: Vec<u8>,
}

impl Argb {
    pub fn new(connection: &RustConnection, screen_number: usize) -> Option<Self> {
        let screen = connection.setup().roots.get(screen_number)?;
        let selection = format!("_NET_WM_CM_S{screen_number}");
        let atom = connection
            .intern_atom(false, selection.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom;
        if connection
            .get_selection_owner(atom)
            .ok()?
            .reply()
            .ok()?
            .owner
            == 0
        {
            return None;
        }
        let formats = connection.render_query_pict_formats().ok()?.reply().ok()?;
        let format = formats.formats.iter().find(|format| {
            let direct = format.direct;
            format.type_ == PictType::DIRECT
                && format.depth == 32
                && direct.red_shift == 16
                && direct.red_mask == 255
                && direct.green_shift == 8
                && direct.green_mask == 255
                && direct.blue_shift == 0
                && direct.blue_mask == 255
                && direct.alpha_shift == 24
                && direct.alpha_mask == 255
        })?;
        let visual = formats
            .screens
            .get(screen_number)?
            .depths
            .iter()
            .flat_map(|depth| &depth.visuals)
            .find(|visual| visual.format == format.id)?
            .visual;
        let colormap = connection.generate_id().ok()?;
        connection
            .create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual)
            .ok()?
            .check()
            .ok()?;
        let pixmap = connection.generate_id().ok()?;
        connection
            .create_pixmap(32, pixmap, screen.root, 1, 1)
            .ok()?
            .check()
            .ok()?;
        let gc = connection.generate_id().ok()?;
        let result = connection
            .create_gc(gc, pixmap, &CreateGCAux::new())
            .ok()?
            .check()
            .ok();
        connection.free_pixmap(pixmap).ok()?;
        result?;
        Some(Self {
            visual,
            colormap,
            gc,
            upload: Vec::new(),
            little_endian: connection.setup().image_byte_order == ImageOrder::LSB_FIRST,
        })
    }

    pub fn blit(
        &mut self,
        connection: &RustConnection,
        window: Window,
        frame: &Frame,
    ) -> Option<()> {
        // PutImage requests are split into rows below the connection's request limit.
        let row_bytes = frame.width as usize * 4;
        let rows = (connection.maximum_request_bytes().saturating_sub(32) / row_bytes).max(1);
        self.upload.resize(frame.pixels.len(), 0);
        self.upload.copy_from_slice(&frame.pixels);
        for pixel in self.upload.chunks_exact_mut(4) {
            if self.little_endian {
                pixel.swap(0, 2);
            } else {
                pixel.rotate_right(1);
            }
        }
        for (index, chunk) in self.upload.chunks(row_bytes * rows).enumerate() {
            connection
                .put_image(
                    ImageFormat::Z_PIXMAP,
                    window,
                    self.gc,
                    u16::try_from(frame.width).ok()?,
                    u16::try_from(chunk.len() / row_bytes).ok()?,
                    0,
                    i16::try_from(index * rows).ok()?,
                    0,
                    32,
                    chunk,
                )
                .ok()?;
        }
        connection.flush().ok()
    }
}

/// Unknown desktops conservatively use static frames, as the old backend did.
pub(super) fn animations_enabled() -> bool {
    use std::process::Command;

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if desktop.contains("gnome") {
        return Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
            .output()
            .is_ok_and(|output| output.status.success() && output.stdout.trim_ascii() == b"true");
    }
    if desktop.contains("kde") {
        for executable in ["kreadconfig6", "kreadconfig5"] {
            if let Ok(output) = Command::new(executable)
                .args([
                    "--file",
                    "kdeglobals",
                    "--group",
                    "KDE",
                    "--key",
                    "AnimationDurationFactor",
                    "--default",
                    "1",
                ])
                .output()
                && output.status.success()
            {
                return std::str::from_utf8(&output.stdout)
                    .ok()
                    .and_then(|text| text.trim().parse::<f64>().ok())
                    .is_some_and(|factor| factor > 0.0);
            }
        }
    }
    false
}
