// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows Firewall problems the daemon reports, with the text that fixes them.

use std::fmt::{self, Display};
use std::path::PathBuf;

use a7n_platform::firewall::{Problem, Profile};
use serde::{Deserialize, Serialize};

/// A Windows Firewall network profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkProfile {
    Domain,
    Private,
    Public,
}

impl From<Profile> for NetworkProfile {
    fn from(profile: Profile) -> Self {
        match profile {
            Profile::Domain => Self::Domain,
            Profile::Private => Self::Private,
            Profile::Public => Self::Public,
        }
    }
}

impl From<NetworkProfile> for Profile {
    fn from(profile: NetworkProfile) -> Self {
        match profile {
            NetworkProfile::Domain => Self::Domain,
            NetworkProfile::Private => Self::Private,
            NetworkProfile::Public => Self::Public,
        }
    }
}

impl Display for NetworkProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&Profile::from(*self), f)
    }
}

/// Something in the Windows Firewall that stops peers from reaching this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FirewallProblem {
    /// No enabled inbound allow rule covers UDP on `ports` for `profiles`.
    MissingAllow {
        program: PathBuf,
        ports: Vec<u16>,
        profiles: Vec<NetworkProfile>,
    },
    /// An enabled inbound block rule matches UDP on `ports`.
    Blocked { rule: String, ports: Vec<u16> },
}

impl From<Problem> for FirewallProblem {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::MissingAllow {
                program,
                ports,
                profiles,
            } => Self::MissingAllow {
                program,
                ports,
                profiles: profiles.iter().map(Into::into).collect(),
            },
            Problem::Blocked { rule, ports } => Self::Blocked { rule, ports },
        }
    }
}

impl FirewallProblem {
    /// An elevated command to add our own allow rule, when its path is paste-safe.
    /// `Blocked` has no command because the rule belongs to someone else.
    #[must_use]
    pub fn fix(&self) -> Option<String> {
        match self {
            Self::MissingAllow {
                program,
                ports,
                profiles,
            } if !program.to_string_lossy().contains(['$', '`', '%', '"']) => Some(format!(
                "netsh advfirewall firewall add rule name=\"Apeirogon\" dir=in action=allow program=\"{}\" protocol=UDP localport={} profile={}",
                program.display(),
                join(ports, ","),
                join(profiles, ","),
            )),
            Self::MissingAllow { .. } | Self::Blocked { .. } => None,
        }
    }
}

impl Display for FirewallProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAllow {
                ports, profiles, ..
            } => write!(
                f,
                "no inbound rule allows apeirogond on UDP ports {} ({} network)",
                join(ports, ", "),
                join(profiles, ", "),
            ),
            Self::Blocked { rule, ports } => write!(
                f,
                "firewall rule \"{rule}\" blocks apeirogond on UDP ports {}",
                join(ports, ", "),
            ),
        }
    }
}

fn join(items: &[impl Display], separator: &str) -> String {
    let items: Vec<String> = items.iter().map(ToString::to_string).collect();
    items.join(separator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_program_path_returns_only_paste_safe_commands() {
        // Arrange
        let cases = [
            (r"C:\Apps\apeirogond.exe", true),
            (r"C:\Program Files (x86)\apeirogond.exe", true),
            (r"C:\$Apps\apeirogond.exe", false),
            (r"C:\`Apps\apeirogond.exe", false),
            (r"C:\%Apps%\apeirogond.exe", false),
            (r#"C:\Apps"\apeirogond.exe"#, false),
        ];
        for (path, expected) in cases {
            let problem = FirewallProblem::MissingAllow {
                program: PathBuf::from(path),
                ports: vec![42042],
                profiles: vec![NetworkProfile::Public],
            };

            // Act
            let fix = problem.fix();

            // Assert
            assert_eq!(fix.is_some(), expected, "{path}");
            if let Some(command) = fix {
                assert!(command.contains(&format!("program=\"{path}\"")));
            }
        }
    }
}
