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
    /// The active network is marked Public.
    PublicNetwork,
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
            Problem::PublicNetwork => Self::PublicNetwork,
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
    /// What the user does about it: a setting to change or a command to run elevated.
    #[must_use]
    pub fn fix(&self) -> String {
        match self {
            Self::PublicNetwork => {
                "Set this network to Private in Windows Settings > Network & internet.".into()
            }
            Self::MissingAllow {
                program,
                ports,
                profiles,
            } => format!(
                "netsh advfirewall firewall add rule name=\"Apeirogon\" dir=in action=allow program=\"{}\" protocol=UDP localport={} profile={}",
                program.display(),
                join(ports, ","),
                join(profiles, ","),
            ),
            Self::Blocked { rule, .. } => format!(
                "netsh advfirewall firewall delete rule name=\"{}\" dir=in",
                rule.replace('"', "\\\""),
            ),
        }
    }
}

impl Display for FirewallProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PublicNetwork => f.write_str("this network is Public, which hides this machine"),
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
                "firewall rule \"{rule}\" blocks UDP ports {}",
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
    fn fix_each_problem_returns_exact_remedy() {
        // Arrange
        let cases = [
            (
                FirewallProblem::PublicNetwork,
                "Set this network to Private in Windows Settings > Network & internet.",
            ),
            (
                FirewallProblem::MissingAllow {
                    program: PathBuf::from(r"C:\Apps\apeirogond.exe"),
                    ports: vec![42042, 5353],
                    profiles: vec![NetworkProfile::Domain, NetworkProfile::Private],
                },
                r#"netsh advfirewall firewall add rule name="Apeirogon" dir=in action=allow program="C:\Apps\apeirogond.exe" protocol=UDP localport=42042,5353 profile=domain,private"#,
            ),
            (
                FirewallProblem::Blocked {
                    rule: r#"apeirogond "x""#.into(),
                    ports: vec![42042],
                },
                r#"netsh advfirewall firewall delete rule name="apeirogond \"x\"" dir=in"#,
            ),
        ];

        for (problem, expected) in cases {
            // Act
            let fix = problem.fix();

            // Assert
            assert_eq!(fix, expected);
        }
    }
}
