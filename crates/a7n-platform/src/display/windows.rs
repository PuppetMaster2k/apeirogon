// SPDX-License-Identifier: GPL-3.0-or-later

use windows_sys::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, GetDeviceCaps, HORZRES, HORZSIZE, VERTRES, VERTSIZE,
};

#[allow(
    clippy::cast_possible_wrap,
    reason = "GDI capability indices are fixed constants that fit in i32"
)]
pub(super) fn physical_size_mm() -> Option<(u32, u32)> {
    let bounds = crate::input::windows::virtual_bounds().ok()?;
    let driver: Vec<u16> = "DISPLAY\0".encode_utf16().collect();
    // SAFETY: driver is NUL-terminated; optional device, port and mode are null.
    let dc = unsafe {
        CreateDCW(
            driver.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if dc.is_null() {
        return None;
    }
    // SAFETY: dc is a live display DC; these queries take no additional pointers.
    let (mm, pixels) = unsafe {
        (
            (
                f64::from(GetDeviceCaps(dc, HORZSIZE as i32)),
                f64::from(GetDeviceCaps(dc, VERTSIZE as i32)),
            ),
            (
                f64::from(GetDeviceCaps(dc, HORZRES as i32)),
                f64::from(GetDeviceCaps(dc, VERTRES as i32)),
            ),
        )
    };
    // SAFETY: dc was created above and is deleted exactly once after its final use.
    unsafe { DeleteDC(dc) };
    super::scale_desktop_mm(
        mm,
        pixels,
        (f64::from(bounds.width), f64::from(bounds.height)),
    )
}
