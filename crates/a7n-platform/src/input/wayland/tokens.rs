// SPDX-License-Identifier: GPL-3.0-or-later

//! The portals' single-use restore tokens, kept in the daemon state directory
//! so that a restart needs no consent dialog.
//!
//! One file, one `name=token` line per portal session. A missing or unreadable
//! file means "no token": the portal then prompts as on first use.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::fs::write_user_only;

const FILE_NAME: &str = "wayland-portal-tokens";
/// Portal tokens are short opaque strings; anything bigger is not ours.
const MAX_FILE_BYTES: u64 = 4096;
const MAX_TOKEN_BYTES: usize = 512;

/// Which portal session a token restores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenKind {
    InputCapture,
    RemoteDesktop,
}

impl TokenKind {
    const ALL: [Self; 2] = [Self::InputCapture, Self::RemoteDesktop];

    fn key(self) -> &'static str {
        match self {
            Self::InputCapture => "input_capture",
            Self::RemoteDesktop => "remote_desktop",
        }
    }
}

#[derive(Debug)]
pub(super) struct TokenStore {
    path: PathBuf,
    input_capture: Option<String>,
    remote_desktop: Option<String>,
}

impl TokenStore {
    /// The store in the per-user daemon state directory, or `None` without a
    /// home directory (sessions then prompt on every start).
    pub(super) fn open_default() -> Option<Self> {
        let dirs = ProjectDirs::from("", "", "apeirogon")?;
        Some(Self::load(dirs.data_local_dir().join(FILE_NAME)))
    }

    pub(super) fn load(path: PathBuf) -> Self {
        let text = read_bounded(&path).unwrap_or_default();
        let mut store = Self {
            path,
            input_capture: None,
            remote_desktop: None,
        };
        for line in text.lines() {
            let Some((key, token)) = line.split_once('=') else {
                continue;
            };
            let Some(kind) = TokenKind::ALL.into_iter().find(|kind| kind.key() == key) else {
                continue;
            };
            *store.slot(kind) = valid_token(token).then(|| token.to_owned());
        }
        store
    }

    pub(super) fn get(&self, kind: TokenKind) -> Option<&str> {
        match kind {
            TokenKind::InputCapture => self.input_capture.as_deref(),
            TokenKind::RemoteDesktop => self.remote_desktop.as_deref(),
        }
    }

    /// Replaces the token (or forgets it with `None`, as a used token is dead)
    /// and writes the file.
    ///
    /// # Errors
    /// Returns the I/O error; the in-memory token is updated either way.
    pub(super) fn set(&mut self, kind: TokenKind, token: Option<&str>) -> io::Result<()> {
        *self.slot(kind) = token.filter(|token| valid_token(token)).map(str::to_owned);
        self.save()
    }

    fn slot(&mut self, kind: TokenKind) -> &mut Option<String> {
        match kind {
            TokenKind::InputCapture => &mut self.input_capture,
            TokenKind::RemoteDesktop => &mut self.remote_desktop,
        }
    }

    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = String::new();
        for kind in TokenKind::ALL {
            if let Some(token) = self.get(kind) {
                text.push_str(kind.key());
                text.push('=');
                text.push_str(token);
                text.push('\n');
            }
        }
        write_user_only(&self.path, text.as_bytes())
    }
}

/// Tokens go into a line format, so they must be one non-empty line.
fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_TOKEN_BYTES
        && !token.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn read_bounded(path: &Path) -> Option<String> {
    let size = fs::metadata(path).ok()?.len();
    if size > MAX_FILE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_load_round_trips_both_tokens() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join(FILE_NAME);
        let mut store = TokenStore::load(path.clone());
        store
            .set(TokenKind::InputCapture, Some("capture-1"))
            .unwrap();
        store
            .set(TokenKind::RemoteDesktop, Some("desktop-1"))
            .unwrap();

        // Act
        let reloaded = TokenStore::load(path);

        // Assert
        assert_eq!(reloaded.get(TokenKind::InputCapture), Some("capture-1"));
        assert_eq!(reloaded.get(TokenKind::RemoteDesktop), Some("desktop-1"));
    }

    #[test]
    fn set_none_forgets_the_used_token_on_disk() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let mut store = TokenStore::load(path.clone());
        store
            .set(TokenKind::InputCapture, Some("capture-1"))
            .unwrap();
        store
            .set(TokenKind::RemoteDesktop, Some("desktop-1"))
            .unwrap();

        // Act
        store.set(TokenKind::InputCapture, None).unwrap();

        // Assert
        let reloaded = TokenStore::load(path);
        assert_eq!(reloaded.get(TokenKind::InputCapture), None);
        assert_eq!(reloaded.get(TokenKind::RemoteDesktop), Some("desktop-1"));
    }

    #[test]
    fn load_corrupt_file_reads_as_no_token() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let garbage: [&[u8]; 3] = [
            b"\xff\xfe\x00binary",
            b"no equals sign\n=\ninput_capture=\n",
            b"input_capture=two words\n",
        ];

        for (index, bytes) in garbage.into_iter().enumerate() {
            let path = dir.path().join(format!("corrupt-{index}"));
            fs::write(&path, bytes).unwrap();

            // Act
            let store = TokenStore::load(path);

            // Assert
            assert_eq!(store.get(TokenKind::InputCapture), None, "case {index}");
            assert_eq!(store.get(TokenKind::RemoteDesktop), None, "case {index}");
        }
    }
}
