// SPDX-License-Identifier: GPL-3.0-or-later

//! Tray glyphs from `icons/` (docs/ui.md section 6), picked per state.
use std::io::Cursor;

use tray_icon::Icon;

use super::State;

/// Which pre-rendered glyph is shown; changes when the state or taskbar theme does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Variant {
    state: State,
    light_taskbar: bool,
}

impl Variant {
    pub(super) fn current(state: State) -> Self {
        Self {
            state,
            light_taskbar: light_taskbar(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("embedded tray icon is malformed")]
    Malformed,
    #[error(transparent)]
    Png(#[from] png::DecodingError),
    #[error(transparent)]
    Bad(#[from] tray_icon::BadIcon),
}

#[cfg(target_os = "macos")]
pub(super) fn load(variant: Variant) -> Result<Icon, IconError> {
    // Template images (black + alpha) so the menu bar tints them; @2x for Retina.
    let bytes: &[u8] = match variant.state {
        State::Idle => include_bytes!("../../icons/macos/tray-idleTemplate@2x.png"),
        State::Remote => include_bytes!("../../icons/macos/tray-remoteTemplate@2x.png"),
        State::Paused => include_bytes!("../../icons/macos/tray-pausedTemplate@2x.png"),
        State::Error => include_bytes!("../../icons/macos/tray-errorTemplate@2x.png"),
    };
    decode_png(bytes)
}

#[cfg(target_os = "windows")]
pub(super) fn load(variant: Variant) -> Result<Icon, IconError> {
    let bytes: &[u8] = match (variant.state, variant.light_taskbar) {
        (State::Idle, false) => include_bytes!("../../icons/windows/tray-idle-dark-taskbar.ico"),
        (State::Idle, true) => include_bytes!("../../icons/windows/tray-idle-light-taskbar.ico"),
        (State::Remote, false) => {
            include_bytes!("../../icons/windows/tray-remote-dark-taskbar.ico")
        }
        (State::Remote, true) => {
            include_bytes!("../../icons/windows/tray-remote-light-taskbar.ico")
        }
        (State::Paused, false) => {
            include_bytes!("../../icons/windows/tray-paused-dark-taskbar.ico")
        }
        (State::Paused, true) => {
            include_bytes!("../../icons/windows/tray-paused-light-taskbar.ico")
        }
        (State::Error, false) => include_bytes!("../../icons/windows/tray-error-dark-taskbar.ico"),
        (State::Error, true) => include_bytes!("../../icons/windows/tray-error-light-taskbar.ico"),
    };
    decode_png(ico_entry(bytes, small_icon_size())?)
}

#[cfg(target_os = "macos")]
fn light_taskbar() -> bool {
    false
}

/// `SystemUsesLightTheme` is the taskbar theme, which can differ from the app theme.
// ponytail: read on every status refresh (about once a second); listen for
// WM_SETTINGCHANGE if that ever shows up in a profile.
#[cfg(target_os = "windows")]
fn light_taskbar() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

    const DWORD_BYTES: u32 = 4;
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value = wide("SystemUsesLightTheme");
    let mut data = 0u32;
    let mut size = DWORD_BYTES;
    // SAFETY: key and value are NUL terminated; data and size point to a
    // writable DWORD and its byte length, as RRF_RT_REG_DWORD requires.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut data).cast(),
            &raw mut size,
        )
    };
    status == 0 && data != 0
}

/// Tray icon edge in pixels at the current DPI.
#[cfg(target_os = "windows")]
fn small_icon_size() -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};

    // SAFETY: GetSystemMetrics has no preconditions.
    u32::try_from(unsafe { GetSystemMetrics(SM_CXSMICON) }).unwrap_or(0)
}

/// The PNG of the smallest ICO entry at least `size` wide, else the largest.
#[cfg(target_os = "windows")]
fn ico_entry(ico: &[u8], size: u32) -> Result<&[u8], IconError> {
    const HEADER: usize = 6;
    const ENTRY: usize = 16;
    // A stored width of 0 means 256.
    const WIDTH_ZERO_MEANS: u32 = 256;
    let field = |e: &[u8], at: usize| u32::from_le_bytes([e[at], e[at + 1], e[at + 2], e[at + 3]]);
    let header = ico.get(4..HEADER).ok_or(IconError::Malformed)?;
    let count = usize::from(u16::from_le_bytes([header[0], header[1]]));
    let entries: Vec<(u32, &[u8])> = (0..count)
        .filter_map(|i| {
            let e = ico.get(HEADER + i * ENTRY..HEADER + (i + 1) * ENTRY)?;
            let width = if e[0] == 0 {
                WIDTH_ZERO_MEANS
            } else {
                u32::from(e[0])
            };
            let len = usize::try_from(field(e, 8)).ok()?;
            let offset = usize::try_from(field(e, 12)).ok()?;
            Some((width, ico.get(offset..offset.checked_add(len)?)?))
        })
        .collect();
    let fitting = entries
        .iter()
        .filter(|(w, _)| *w >= size)
        .min_by_key(|(w, _)| *w);
    let largest = || entries.iter().max_by_key(|(w, _)| *w);
    fitting
        .or_else(largest)
        .map(|(_, data)| *data)
        .ok_or(IconError::Malformed)
}

fn decode_png(bytes: &[u8]) -> Result<Icon, IconError> {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info()?;
    let mut rgba = vec![0; reader.output_buffer_size().ok_or(IconError::Malformed)?];
    let info = reader.next_frame(&mut rgba)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(IconError::Malformed);
    }
    rgba.truncate(info.buffer_size());
    Ok(Icon::from_rgba(rgba, info.width, info.height)?)
}
