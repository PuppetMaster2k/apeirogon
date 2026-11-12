// SPDX-License-Identifier: GPL-3.0-or-later

//! A reusable top-down premultiplied BGRA DIB for a layered window.

use std::ffi::c_void;
use std::ptr::{copy_nonoverlapping, null_mut};

use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SW_SHOWNOACTIVATE, ShowWindow, ULW_ALPHA, UpdateLayeredWindow,
};

use super::super::raster::Frame;
use super::geometry::Rect;

#[derive(Default)]
pub(super) struct Bitmap {
    dc: HDC,
    dib: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut c_void,
    size: (u32, u32),
}

impl Bitmap {
    fn resize(&mut self, width: u32, height: u32) -> Option<()> {
        if self.size == (width, height) {
            return Some(());
        }
        *self = Self::default();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).ok()?,
                biWidth: i32::try_from(width).ok()?,
                biHeight: -i32::try_from(height).ok()?,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        // SAFETY: The DIB owns its pixels until Drop; all handles stay on this UI thread.
        unsafe {
            self.dc = CreateCompatibleDC(null_mut());
            if self.dc.is_null() {
                return None;
            }
            self.dib = CreateDIBSection(
                self.dc,
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut self.bits,
                null_mut(),
                0,
            );
            if self.dib.is_null() {
                return None;
            }
            self.previous = SelectObject(self.dc, self.dib);
        }
        self.size = (width, height);
        Some(())
    }

    pub fn show(&mut self, window: HWND, rect: Rect, frame: &Frame) {
        if self.resize(frame.width, frame.height).is_none() {
            return;
        }
        // SAFETY: The DIB has exactly frame.pixels.len() bytes, is uniquely owned,
        // and remains selected into this DC throughout UpdateLayeredWindow.
        unsafe {
            copy_nonoverlapping(frame.pixels.as_ptr(), self.bits.cast(), frame.pixels.len());
            let bytes = std::slice::from_raw_parts_mut(self.bits.cast::<u8>(), frame.pixels.len());
            for pixel in bytes.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            let point = POINT {
                x: rect.left,
                y: rect.top,
            };
            let source = POINT::default();
            let size = SIZE {
                cx: rect.width(),
                cy: rect.height(),
            };
            let blend = BLENDFUNCTION {
                BlendOp: u8::try_from(AC_SRC_OVER).unwrap_or(0),
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: u8::try_from(AC_SRC_ALPHA).unwrap_or(1),
            };
            if UpdateLayeredWindow(
                window,
                null_mut(),
                &raw const point,
                &raw const size,
                self.dc,
                &raw const source,
                0,
                &raw const blend,
                ULW_ALPHA,
            ) != 0
            {
                ShowWindow(window, SW_SHOWNOACTIVATE);
            }
        }
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: Restore the original object before deleting our DIB and DC.
        unsafe {
            if !self.previous.is_null() {
                SelectObject(self.dc, self.previous);
            }
            if !self.dib.is_null() {
                DeleteObject(self.dib);
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
        }
    }
}
