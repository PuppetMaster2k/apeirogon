// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Command, Path, PathBuf, io, remove, run};

fn path() -> io::Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| {
            dirs.home_dir()
                .join("Library/LaunchAgents/dev.apeirogon.daemon.plist")
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory found"))
}

fn plist(exe: &Path) -> String {
    let exe = exe
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>dev.apeirogon.daemon</string>
<key>ProgramArguments</key><array><string>{exe}</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
</dict></plist>
"#
    )
}

fn domain() -> String {
    format!("gui/{}", rustix::process::getuid().as_raw())
}

fn loaded() -> io::Result<bool> {
    Ok(Command::new("launchctl")
        .arg("print")
        .arg(format!("{}/dev.apeirogon.daemon", domain()))
        .output()?
        .status
        .success())
}

pub(super) fn enable(exe: &Path) -> io::Result<Option<&'static str>> {
    let path = path()?;
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("plist has no parent"))?,
    )?;
    if loaded()? {
        run(Command::new("launchctl")
            .arg("bootout")
            .arg(domain())
            .arg(&path))?;
    }
    crate::fs::write_user_only(&path, plist(exe).as_bytes())?;
    run(Command::new("launchctl")
        .arg("bootstrap")
        .arg(domain())
        .arg(path))?;
    Ok(None)
}

pub(super) fn disable() -> io::Result<Option<&'static str>> {
    let path = path()?;
    if loaded()? {
        run(Command::new("launchctl")
            .arg("bootout")
            .arg(domain())
            .arg(&path))?;
    }
    remove(&path)?;
    Ok(None)
}

pub(super) fn is_enabled() -> io::Result<bool> {
    Ok(path()?.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_executable_path_is_xml_escaped() {
        // Arrange
        let exe = Path::new("/Users/me/My & App/apeirogond");

        // Act
        let text = plist(exe);

        // Assert
        assert!(text.contains("<string>/Users/me/My &amp; App/apeirogond</string>"));
    }
}
