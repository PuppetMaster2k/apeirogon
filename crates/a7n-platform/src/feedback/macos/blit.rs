// SPDX-License-Identifier: GPL-3.0-or-later

//! A reusable bitmap backing for the panel's `CGImage` contents.

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage, NSImageView};
use objc2_foundation::NSSize;

use super::super::raster::Frame;

#[derive(Default)]
pub(super) struct Bitmap {
    rep: Option<Retained<NSBitmapImageRep>>,
    size: (u32, u32),
}

impl Bitmap {
    pub fn show(&mut self, view: &NSImageView, frame: &Frame) {
        if self.size != (frame.width, frame.height) {
            let (Ok(width), Ok(height)) =
                (isize::try_from(frame.width), isize::try_from(frame.height))
            else {
                return;
            };
            // SAFETY: Null planes asks AppKit to own the storage. The RGBA layout
            // is premultiplied, interleaved, eight bits per channel.
            self.rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(), std::ptr::null_mut(),
                    width, height,
                    8, 4, true, false, NSDeviceRGBColorSpace,
                    width * 4, 32,
                )
            };
            if self.rep.is_none() {
                return;
            }
            self.size = (frame.width, frame.height);
        }
        let Some(rep) = self.rep.as_ref() else {
            return;
        };
        let pixels = rep.bitmapData();
        if pixels.is_null() || !frame.is_drawable() {
            return;
        }
        // SAFETY: The representation owns exactly this many writable RGBA bytes;
        // both it and the source frame are exclusively accessed on the UI thread.
        unsafe {
            std::ptr::copy_nonoverlapping(frame.pixels.as_ptr(), pixels, frame.pixels.len());
        }
        let Some(cg_image) = rep.CGImage() else {
            return;
        };
        let image = NSImage::initWithCGImage_size(
            NSImage::alloc(),
            &cg_image,
            NSSize::new(f64::from(frame.width), f64::from(frame.height)),
        );
        view.setImage(Some(&image));
    }
}
