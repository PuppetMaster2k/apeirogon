// SPDX-License-Identifier: GPL-3.0-or-later

use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyW,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

use super::{Path, io};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn command(exe: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::iter::once(u16::from(b'"'))
        .chain(exe.as_os_str().encode_wide())
        .chain([u16::from(b'"'), 0])
        .collect()
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by RegOpenKeyExW and is closed once.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn open(access: u32) -> io::Result<Key> {
    let mut key = ptr::null_mut();
    let name = wide(RUN_KEY);
    // SAFETY: name is NUL terminated and key points to a writable handle slot.
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, access, &raw mut key) };
    check(status)?;
    Ok(Key(key))
}

fn check(status: u32) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(i32::from_ne_bytes(
            status.to_ne_bytes(),
        )))
    }
}

pub(super) fn enable(exe: &Path) -> io::Result<Option<&'static str>> {
    let mut handle = ptr::null_mut();
    let run = wide(RUN_KEY);
    // SAFETY: run is NUL terminated and handle points to a writable handle slot.
    check(unsafe { RegCreateKeyW(HKEY_CURRENT_USER, run.as_ptr(), &raw mut handle) })?;
    let key = Key(handle);
    let name = wide("apeirogond");
    let command = command(exe);
    let bytes = u32::try_from(command.len() * size_of::<u16>()).map_err(io::Error::other)?;
    // SAFETY: key is live, name is terminated, command holds bytes UTF-16 data including NUL.
    check(unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_SZ,
            command.as_ptr().cast(),
            bytes,
        )
    })?;
    Ok(None)
}

pub(super) fn disable() -> io::Result<Option<&'static str>> {
    let key = match open(KEY_SET_VALUE) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let name = wide("apeirogond");
    // SAFETY: key is live and name is NUL terminated.
    let status = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
    if status != ERROR_FILE_NOT_FOUND {
        check(status)?;
    }
    Ok(None)
}

pub(super) fn is_enabled() -> io::Result<bool> {
    let key = match open(KEY_QUERY_VALUE) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let name = wide("apeirogond");
    // SAFETY: queries existence only; optional output pointers may be null.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    check(status)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_executable_path_is_quoted_and_terminated() {
        // Arrange
        let exe = Path::new(r"C:\Program Files\Apeirogon\apeirogond.exe");

        // Act
        let text = command(exe);

        // Assert
        assert_eq!(text, wide(r#""C:\Program Files\Apeirogon\apeirogond.exe""#));
    }
}
