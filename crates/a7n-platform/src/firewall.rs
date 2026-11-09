// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows Firewall diagnostics: reports why peers may not be able to reach
//! this machine. Reading the firewall needs no elevation, and nothing here
//! changes it; the fix is for the user to run elevated.

use std::fmt;
use std::path::PathBuf;

#[cfg(any(windows, test))]
mod rules;
#[cfg(windows)]
mod windows;

/// A network profile of the Windows Firewall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Domain,
    Private,
    Public,
}

impl Profile {
    const ALL: [Self; 3] = [Self::Domain, Self::Private, Self::Public];

    /// The value `netsh advfirewall` takes for `profile=`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Private => "private",
            Self::Public => "public",
        }
    }

    /// The `NET_FW_PROFILE_TYPE2` bit of this profile.
    fn bit(self) -> u32 {
        match self {
            Self::Domain => 1,
            Self::Private => 2,
            Self::Public => 4,
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of [`Profile`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Profiles(u32);

impl Profiles {
    pub fn iter(self) -> impl Iterator<Item = Profile> {
        Profile::ALL
            .into_iter()
            .filter(move |&profile| self.has(profile))
    }

    fn has(self, profile: Profile) -> bool {
        self.0 & profile.bit() != 0
    }
}

/// Something that stops peers from reaching this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No enabled inbound allow rule covers UDP on `ports` for `profiles`.
    MissingAllow {
        program: PathBuf,
        ports: Vec<u16>,
        profiles: Profiles,
    },
    /// An enabled inbound block rule matches UDP on `ports`; blocks win over allows.
    Blocked { rule: String, ports: Vec<u16> },
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("cannot find the path of this program: {0}")]
    Program(#[from] std::io::Error),
    #[error("the firewall could not be queried (HRESULT {0:#010x})")]
    Query(i32),
}

/// Whether this platform has a firewall backend; [`check`] is pointless otherwise.
pub const SUPPORTED: bool = cfg!(windows);

/// Problems that block links on UDP `port` and mDNS, checked live on every
/// call. Blocks on COM calls: run it off the async runtime. Always empty off
/// Windows.
///
/// # Errors
///
/// [`CheckError`] when this program's path or the firewall cannot be read.
pub fn check(port: u16) -> Result<Vec<Problem>, CheckError> {
    #[cfg(windows)]
    return windows::check(port);
    #[cfg(not(windows))]
    {
        let _ = port;
        Ok(Vec::new())
    }
}
