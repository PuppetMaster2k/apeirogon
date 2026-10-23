// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-user start-at-login integration.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as backend;

/// Installs and starts the per-user login service.
/// Returns a notice when the service manager is unavailable.
///
/// # Errors
/// Returns file or service-manager errors.
pub fn enable(exe: &Path) -> io::Result<Option<&'static str>> {
    backend::enable(exe)
}

/// Disables and removes the per-user login service.
///
/// # Errors
/// Returns file or service-manager errors.
pub fn disable() -> io::Result<Option<&'static str>> {
    backend::disable()
}

/// Whether the per-user login service is installed and enabled.
///
/// # Errors
/// Returns file or service-manager errors.
pub fn is_enabled() -> io::Result<bool> {
    backend::is_enabled()
}

/// Finds the daemon beside the CLI, falling back to PATH.
///
/// # Errors
/// Returns an error when the current executable cannot be located.
pub fn daemon_executable() -> io::Result<PathBuf> {
    let sibling = std::env::current_exe()?
        .with_file_name(format!("apeirogond{}", std::env::consts::EXE_SUFFIX));
    Ok(if sibling.is_file() {
        sibling
    } else {
        PathBuf::from(format!("apeirogond{}", std::env::consts::EXE_SUFFIX))
    })
}

/// Spawns a detached daemon with no inherited terminal streams.
///
/// # Errors
/// Returns any process spawn error.
pub fn start(exe: &Path) -> io::Result<()> {
    let mut command = Command::new(exe);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches no Rust state after fork.
        unsafe {
            command.pre_exec(|| {
                rustix::process::setsid()
                    .map(|_| ())
                    .map_err(io::Error::from)
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
    command.spawn()?;
    Ok(())
}

#[cfg(unix)]
fn remove(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(unix)]
fn run(command: &mut Command) -> io::Result<()> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(
        String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    ))
}
