// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure decision logic over firewall rules read from the OS.

use std::path::{Path, PathBuf};

use super::{Problem, Profiles};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Allow,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Protocol {
    Udp,
    Tcp,
    Any,
    Other,
}

#[derive(Debug, Clone)]
pub(super) struct Rule {
    pub name: String,
    pub enabled: bool,
    pub direction: Direction,
    pub action: Action,
    pub protocol: Protocol,
    /// As Windows reports it: `*`, `42042`, `1000-2000,5353`, or a keyword.
    pub local_ports: String,
    /// None means every program.
    pub program: Option<PathBuf>,
    pub profiles: Profiles,
    pub local_users: String,
    pub package_id: String,
    pub remote_addresses: String,
}

impl Profiles {
    fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// Problems for `program` on UDP `ports` while the firewall is on for the
/// `active` profiles; nothing when no profile is active.
pub(super) fn evaluate(
    rules: &[Rule],
    active: Profiles,
    program: &Path,
    ports: &[u16],
    user_sid: &str,
) -> Vec<Problem> {
    if active.is_empty() {
        return Vec::new();
    }
    let relevant: Vec<&Rule> = rules
        .iter()
        .filter(|rule| rule.reaches(active, program, user_sid))
        .collect();
    let mut problems = Vec::new();
    problems.extend(blocking(&relevant, ports));
    problems.extend(missing_allow(&relevant, active, program, ports));
    problems
}

impl Rule {
    /// An enabled inbound UDP rule for this program on one of the active profiles.
    fn reaches(&self, active: Profiles, program: &Path, user_sid: &str) -> bool {
        let inbound = self.direction == Direction::Inbound;
        let udp = matches!(self.protocol, Protocol::Udp | Protocol::Any);
        let active_profile = self.profiles.intersects(active);
        let unpackaged = self.package_id.trim().is_empty();
        let user_matches = self.applies_to_user(user_sid);
        let program_matches = self
            .program
            .as_deref()
            .is_none_or(|own| same_program(own, program));
        self.enabled
            && inbound
            && udp
            && active_profile
            && unpackaged
            && user_matches
            && program_matches
    }

    fn applies_to_user(&self, user_sid: &str) -> bool {
        if is_unrestricted(&self.local_users) {
            return true;
        }
        let mut allowed = false;
        for entry in self.local_users.split('(').skip(1) {
            let Some((ace, _)) = entry.split_once(')') else {
                continue;
            };
            let mut fields = ace.split(';');
            let kind = fields.next();
            let trustee = fields.nth(4);
            if !trustee.is_some_and(|sid| sid.eq_ignore_ascii_case(user_sid)) {
                continue;
            }
            match kind {
                Some("D") => return false,
                Some("A") => allowed = true,
                _ => {}
            }
        }
        allowed
    }

    fn covers(&self, port: u16) -> bool {
        covers_port(&self.local_ports, port)
    }
}

fn is_unrestricted(spec: &str) -> bool {
    matches!(spec.trim(), "" | "*")
}

fn blocking(relevant: &[&Rule], ports: &[u16]) -> Vec<Problem> {
    relevant
        .iter()
        .filter(|rule| rule.action == Action::Block)
        // A scoped block may leave other peers reachable, so it is not a blanket block.
        .filter(|rule| is_unrestricted(&rule.remote_addresses))
        .filter_map(|rule| {
            let blocked: Vec<u16> = ports
                .iter()
                .copied()
                .filter(|&port| rule.covers(port))
                .collect();
            (!blocked.is_empty()).then(|| Problem::Blocked {
                rule: rule.name.clone(),
                ports: blocked,
            })
        })
        .collect()
}

fn missing_allow(
    relevant: &[&Rule],
    active: Profiles,
    program: &Path,
    ports: &[u16],
) -> Option<Problem> {
    let mut missing_ports = Vec::new();
    let mut missing_profiles = Profiles::default();
    for &port in ports {
        let allowed = relevant
            .iter()
            .filter(|rule| rule.action == Action::Allow && rule.covers(port))
            .fold(Profiles::default(), |all, rule| all.union(rule.profiles));
        let uncovered = active.without(allowed);
        if !uncovered.is_empty() {
            missing_ports.push(port);
            missing_profiles = missing_profiles.union(uncovered);
        }
    }
    (!missing_ports.is_empty()).then(|| Problem::MissingAllow {
        program: program.to_path_buf(),
        ports: missing_ports,
        profiles: missing_profiles,
    })
}

/// Windows paths compare case-insensitively. Rule paths with environment
/// variables such as `%SystemRoot%` are not expanded and never match.
fn same_program(rule: &Path, program: &Path) -> bool {
    normalize(rule) == normalize(program)
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Accepts `*`, `Any`, an empty list, single ports and ranges, comma
/// separated. Anything else, such as the keyword `RPC`, matches nothing.
fn covers_port(spec: &str, port: u16) -> bool {
    let spec = spec.trim();
    if spec.is_empty() || spec == "*" || spec.eq_ignore_ascii_case("any") {
        return true;
    }
    spec.split(',')
        .any(|item| match item.trim().split_once('-') {
            Some((low, high)) => matches!(
                (low.trim().parse::<u16>(), high.trim().parse::<u16>()),
                (Ok(low), Ok(high)) if (low..=high).contains(&port)
            ),
            None => item.trim().parse() == Ok(port),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER_SID: &str = "S-1-5-21-123-456-789-1001";
    const PORT: u16 = 42042;
    const MDNS: u16 = 5353;
    const PRIVATE: Profiles = Profiles(2);
    const PUBLIC: Profiles = Profiles(4);
    const ALL_PROFILES: Profiles = Profiles(0x7FFF_FFFF);

    fn program() -> PathBuf {
        PathBuf::from(r"C:\Apps\apeirogond.exe")
    }

    fn rule(action: Action, ports: &str, profiles: Profiles) -> Rule {
        Rule {
            name: "test rule".into(),
            enabled: true,
            direction: Direction::Inbound,
            action,
            protocol: Protocol::Udp,
            local_ports: ports.into(),
            program: Some(program()),
            profiles,
            local_users: String::new(),
            package_id: String::new(),
            remote_addresses: "*".into(),
        }
    }

    fn evaluate_private(rules: &[Rule]) -> Vec<Problem> {
        evaluate(rules, PRIVATE, &program(), &[PORT, MDNS], USER_SID)
    }

    #[test]
    fn covers_port_specs_match_windows_syntax() {
        // Arrange
        let cases = [
            ("*", PORT, true),
            ("Any", PORT, true),
            ("42042", PORT, true),
            ("1000-2000,5353", MDNS, true),
            ("1000-2000,5353", PORT, false),
            ("42000-42100", PORT, true),
            ("80, 443", PORT, false),
            ("RPC", PORT, false),
            ("9-x", PORT, false),
        ];

        for (spec, port, expected) in cases {
            // Act
            let result = covers_port(spec, port);

            // Assert
            assert_eq!(result, expected, "{spec} on {port}");
        }
    }

    #[test]
    fn evaluate_program_path_differs_in_case_matches() {
        // Arrange
        let mut allow = rule(Action::Allow, "*", ALL_PROFILES);
        allow.program = Some(PathBuf::from(r"c:/apps/APEIROGOND.EXE"));

        // Act
        let problems = evaluate_private(&[allow]);

        // Assert
        assert_eq!(problems, Vec::new());
    }

    #[test]
    fn evaluate_no_rules_reports_missing_allow_for_both_ports() {
        // Arrange
        let expected = Problem::MissingAllow {
            program: program(),
            ports: vec![PORT, MDNS],
            profiles: PRIVATE,
        };

        // Act
        let problems = evaluate_private(&[]);

        // Assert
        assert_eq!(problems, vec![expected]);
    }

    #[test]
    fn evaluate_rule_for_other_profile_reports_missing_allow() {
        // Arrange
        let rules = [rule(Action::Allow, "*", PUBLIC)];

        // Act
        let problems = evaluate_private(&rules);

        // Assert
        assert!(matches!(
            problems.as_slice(),
            [Problem::MissingAllow { .. }]
        ));
    }

    #[test]
    fn evaluate_unusable_rules_are_ignored() {
        // Arrange
        let mut disabled = rule(Action::Allow, "*", ALL_PROFILES);
        disabled.enabled = false;
        let mut outbound = rule(Action::Allow, "*", ALL_PROFILES);
        outbound.direction = Direction::Outbound;
        let mut tcp = rule(Action::Allow, "*", ALL_PROFILES);
        tcp.protocol = Protocol::Tcp;
        let mut icmp = rule(Action::Allow, "*", ALL_PROFILES);
        icmp.protocol = Protocol::Other;
        let mut other_program = rule(Action::Allow, "*", ALL_PROFILES);
        other_program.program = Some(PathBuf::from(r"C:\Other\app.exe"));

        // Act
        let problems = evaluate_private(&[disabled, outbound, tcp, icmp, other_program]);

        // Assert
        assert!(matches!(
            problems.as_slice(),
            [Problem::MissingAllow { .. }]
        ));
    }

    #[test]
    fn evaluate_allow_rule_for_any_program_counts() {
        // Arrange
        let mut allow = rule(Action::Allow, "*", ALL_PROFILES);
        allow.program = None;
        allow.protocol = Protocol::Any;

        // Act
        let problems = evaluate_private(&[allow]);

        // Assert
        assert_eq!(problems, Vec::new());
    }

    #[test]
    fn evaluate_block_rule_wins_over_allow_rule() {
        // Arrange
        let allow = rule(Action::Allow, "*", ALL_PROFILES);
        let mut block = rule(Action::Block, "42000-42100", PRIVATE);
        block.name = "Dismissed prompt".into();

        // Act
        let problems = evaluate_private(&[allow, block]);

        // Assert
        let expected = Problem::Blocked {
            rule: "Dismissed prompt".into(),
            ports: vec![PORT],
        };
        assert_eq!(problems, vec![expected]);
    }

    #[test]
    fn evaluate_firewall_off_for_current_profile_reports_nothing() {
        // Arrange
        let no_active_profile = Profiles::default();

        // Act
        let problems = evaluate(&[], no_active_profile, &program(), &[PORT, MDNS], USER_SID);

        // Assert
        assert_eq!(problems, Vec::new());
    }

    #[test]
    fn evaluate_block_for_other_user_reports_nothing() {
        // Arrange
        let allow = rule(Action::Allow, "*", PRIVATE);
        let mut block = rule(Action::Block, "*", PRIVATE);
        block.local_users = format!("D:(A;;CC;;;{USER_SID}0)");

        // Act
        let problems = evaluate_private(&[allow, block]);

        // Assert
        assert_eq!(problems, Vec::new());
    }

    #[test]
    fn evaluate_block_listing_current_user_reports_blocked() {
        // Arrange
        let allow = rule(Action::Allow, "*", PRIVATE);
        let mut block = rule(Action::Block, "*", PRIVATE);
        block.local_users = format!("D:(A;;CC;;;S-1-5-18)(A;;CC;;;{USER_SID})");
        block.remote_addresses = String::new();

        // Act
        let problems = evaluate_private(&[allow, block]);

        // Assert
        assert!(matches!(problems.as_slice(), [Problem::Blocked { .. }]));
    }

    #[test]
    fn evaluate_block_with_specific_remote_addresses_reports_nothing() {
        // Arrange
        let addresses = ["LocalSubnet", "192.0.2.1", "10.0.0.0/8"];
        for address in addresses {
            let allow = rule(Action::Allow, "*", PRIVATE);
            let mut block = rule(Action::Block, "*", PRIVATE);
            block.remote_addresses = address.into();

            // Act
            let problems = evaluate_private(&[allow, block]);

            // Assert
            assert!(problems.is_empty(), "{address}");
        }
    }

    #[test]
    fn evaluate_allow_for_other_user_reports_missing_allow() {
        // Arrange
        let mut allow = rule(Action::Allow, "*", PRIVATE);
        allow.local_users = "D:(A;;CC;;;S-1-5-18)".into();

        // Act
        let problems = evaluate_private(&[allow]);

        // Assert
        assert!(matches!(
            problems.as_slice(),
            [Problem::MissingAllow { .. }]
        ));
    }

    #[test]
    fn applies_to_user_deny_ace_overrides_allow() {
        // Arrange
        let cases = [
            format!("D:(A;;CC;;;{USER_SID})(D;;CC;;;{USER_SID})"),
            format!("D:(D;;CC;;;{USER_SID})(A;;CC;;;{USER_SID})"),
            format!("D:(D;;CC;;;{USER_SID})"),
        ];
        for sddl in cases {
            let mut scoped = rule(Action::Block, "*", PRIVATE);
            scoped.local_users = sddl;

            // Act
            let applies = scoped.applies_to_user(USER_SID);

            // Assert
            assert!(!applies);
        }
    }

    #[test]
    fn evaluate_package_scoped_rules_do_not_apply() {
        // Arrange
        for action in [Action::Allow, Action::Block] {
            let mut scoped = rule(action, "*", PRIVATE);
            scoped.package_id = "S-1-15-2-123".into();

            // Act
            let problems = evaluate_private(&[scoped]);

            // Assert
            assert!(matches!(
                problems.as_slice(),
                [Problem::MissingAllow { .. }]
            ));
        }
    }
}
