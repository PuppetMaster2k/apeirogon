// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Command, Path, PathBuf, io, remove, run};

const SERVICE: &str = "apeirogond.service";
const MISSING: &str = "systemctl is unavailable; the user unit was updated but cannot be enabled or stopped automatically";

fn path() -> io::Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.config_dir().join("systemd/user").join(SERVICE))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory found"))
}

fn unit(exe: &Path) -> String {
    let exe = exe
        .to_string_lossy()
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('%', "%%")
        .replace('$', "$$")
        .replace('\n', r"\n")
        .replace('\r', r"\r");
    format!(
        "[Unit]\nDescription=Apeirogon session daemon\n\n[Service]\nExecStart=\"{exe}\"\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n"
    )
}

fn systemctl(args: &[&str]) -> io::Result<Option<&'static str>> {
    match run(Command::new("systemctl").arg("--user").args(args)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Some(MISSING)),
        result => result.map(|()| None),
    }
}

pub(super) fn enable(exe: &Path) -> io::Result<Option<&'static str>> {
    let path = path()?;
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("unit has no parent"))?,
    )?;
    crate::fs::write_user_only(&path, unit(exe).as_bytes())?;
    if let Some(notice) = systemctl(&["daemon-reload"])? {
        return Ok(Some(notice));
    }
    systemctl(&["enable", "--now", SERVICE])
}

pub(super) fn disable() -> io::Result<Option<&'static str>> {
    let notice = systemctl(&["disable", "--now", SERVICE])?;
    remove(&path()?)?;
    if notice.is_some() {
        return Ok(notice);
    }
    systemctl(&["daemon-reload"])
}

pub(super) fn is_enabled() -> io::Result<bool> {
    if !path()?.exists() {
        return Ok(false);
    }
    match Command::new("systemctl")
        .args(["--user", "is-enabled", SERVICE])
        .output()
    {
        Ok(output) => Ok(output.status.success()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_executable_path_is_quoted_and_specifiers_escaped() {
        // Arrange
        let exe = Path::new("/home/user/my app/apeirogond%test");

        // Act
        let text = unit(exe);

        // Assert
        assert!(text.contains("ExecStart=\"/home/user/my app/apeirogond%%test\""));
        assert!(text.contains("WantedBy=default.target"));
    }
}
