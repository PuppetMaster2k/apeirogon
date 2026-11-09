// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{
    ErrorKind, FileTransfer, FinishedSend, HeldFiles, PeerInfo, SendOutcome, StatusInfo,
    TransferDirection,
};

use super::*;

#[test]
fn parse_addr_ip_and_port_forms_resolve_or_reject() {
    // Arrange
    let cases = [
        (
            "127.0.0.1",
            Some(SocketAddr::from(([127, 0, 0, 1], DEFAULT_UDP_PORT))),
        ),
        (
            "127.0.0.1:1234",
            Some(SocketAddr::from(([127, 0, 0, 1], 1234))),
        ),
        (
            "::1",
            Some(SocketAddr::new(
                IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
                DEFAULT_UDP_PORT,
            )),
        ),
        (
            "[::1]:1234",
            Some(SocketAddr::new(
                IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
                1234,
            )),
        ),
        (
            "2001:db8::1234",
            Some(SocketAddr::new(
                "2001:db8::1234".parse().unwrap(),
                DEFAULT_UDP_PORT,
            )),
        ),
        ("", None),
        ("localhost:1234", None),
        ("999.0.0.1", None),
        ("127.0.0.1:65536", None),
        ("[::1]:no", None),
        ("[::1]", None),
    ];
    for (input, expected) in cases {
        // Act
        let result = parse_addr(input);

        // Assert
        assert_eq!(result.ok(), expected, "{input}");
    }
}

#[test]
fn write_response_status_displays_identity_and_peer_states() {
    // Arrange
    let machine = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let response = Response::Status(StatusInfo {
        held_clipboard: None,
        transfers: vec![FileTransfer {
            id: 7,
            offer_id: Some(2),
            peer: machine,
            direction: TransferDirection::Send,
            files_done: 1,
            files_total: 3,
            bytes_done: 1536,
            bytes_total: 5 * 1024 * 1024,
        }],
        finished_sends: Vec::new(),
        held_files: vec![HeldFiles {
            peer: machine,
            offer_id: 1,
            files: 2,
            top_level_entries: 1,
            total_size: 100,
        }],
        input: Some("Accessibility permission missing".to_owned()),
        paused: false,
        focused: machine,
        pid: std::process::id(),
        machine,
        name: "local".into(),
        peers: [true, false]
            .into_iter()
            .map(|connected| PeerInfo {
                id: machine,
                name: "peer".into(),
                connected,
                rtt_ms: None,
                can_wake: false,
                waking: false,
            })
            .collect(),
        nearby: Vec::new(),
        missing_permissions: vec![a7n_ipc::Permission::Accessibility],
        firewall: vec![a7n_ipc::FirewallProblem::Blocked {
            rule: "managed rule".into(),
            ports: vec![42042],
        }],
    });
    let mut output = Vec::new();

    // Act
    write_response(&mut output, &response).unwrap();

    // Assert
    assert_eq!(
        String::from_utf8(output).unwrap(),
        concat!(
            "input: unavailable (Accessibility permission missing)\n",
            "permission missing: Accessibility\n",
            "firewall: firewall rule \"managed rule\" blocks apeirogond on UDP ports 42042\n",
            "local 0123456789abcdef0123456789abcdef (this machine)\n",
            "peer 0123456789abcdef0123456789abcdef connected\n",
            "peer 0123456789abcdef0123456789abcdef offline\n",
            "files waiting for confirmation: peer wants to send 2 files, 100 B (a7n files accept 0123456789abcdef0123456789abcdef)\n",
            "transfer 7 send peer: 1/3 files, 1.5 KiB / 5.0 MiB\n",
        )
    );
}

#[test]
fn resolve_target_addresses_and_nearby_prefixes_resolve_or_reject() {
    // Arrange
    let first_addr = "127.0.0.1:1234".parse().unwrap();
    let second_addr = "[::1]:4321".parse().unwrap();
    let nearby = [
        NearbyMachine {
            id: "abcd456789abcdef0123456789abcdef".parse().unwrap(),
            addr: first_addr,
        },
        NearbyMachine {
            id: "abcd556789abcdef0123456789abcdef".parse().unwrap(),
            addr: second_addr,
        },
    ];
    let invalid = "expected an IP address, IP:port, or machine id prefix of 4 to 32 hex digits";
    let cases = [
        ("127.0.0.1:1234", Ok(first_addr)),
        (
            "127.0.0.1",
            Ok(SocketAddr::from(([127, 0, 0, 1], DEFAULT_UDP_PORT))),
        ),
        ("[::1]:4321", Ok(second_addr)),
        ("abcd456789abcdef0123456789abcdef", Ok(first_addr)),
        ("abcd4", Ok(first_addr)),
        ("ABCD5", Ok(second_addr)),
        (
            "abcd",
            Err("ambiguous nearby machine id prefix; use more hex digits"),
        ),
        ("ffff", Err("no nearby machine matches that id prefix")),
        ("abc", Err(invalid)),
        ("", Err(invalid)),
        ("xyz0", Err(invalid)),
        ("éabc", Err(invalid)),
        ("abcd456789abcdef0123456789abcdef0", Err(invalid)),
    ];
    for (target, expected) in cases {
        // Act
        let result = resolve_target(target, &nearby);

        // Assert
        assert_eq!(result, expected, "{target}");
    }
}

#[test]
fn write_response_status_with_nearby_displays_machine_ids_and_addresses() {
    // Arrange
    let machine = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let response = Response::Status(StatusInfo {
        held_clipboard: None,
        transfers: Vec::new(),
        held_files: Vec::new(),
        finished_sends: Vec::new(),
        input: None,
        paused: false,
        focused: machine,
        pid: std::process::id(),
        machine,
        name: "local".into(),
        peers: Vec::new(),
        nearby: vec![NearbyMachine {
            id: machine,
            addr: "127.0.0.1:1234".parse().unwrap(),
        }],
        missing_permissions: Vec::new(),
        firewall: Vec::new(),
    });
    let mut output = Vec::new();

    // Act
    write_response(&mut output, &response).unwrap();

    // Assert
    assert_eq!(
        String::from_utf8(output).unwrap(),
        concat!(
            "local 0123456789abcdef0123456789abcdef (this machine)\n",
            "nearby\n",
            "0123456789abcdef0123456789abcdef 127.0.0.1:1234\n",
        )
    );
}

#[test]
fn write_response_pairing_and_completion_display_expected_output() {
    // Arrange
    let cases = [
        (
            Response::PairCode {
                code: PairingCode::parse("001234").unwrap(),
                expires_in_secs: 60,
            },
            "001234 (valid for 60 s)\n",
        ),
        (
            Response::Paired {
                machine: "0123456789abcdef0123456789abcdef".parse().unwrap(),
                name: "peer".into(),
            },
            "Paired with peer (0123456789abcdef0123456789abcdef)\n",
        ),
        (
            Response::Waking {
                name: "desktop".into(),
            },
            "Waking desktop…\n",
        ),
        (Response::Done, ""),
    ];
    for (response, expected) in cases {
        let mut output = Vec::new();

        // Act
        write_response(&mut output, &response).unwrap();

        // Assert
        assert_eq!(output, expected.as_bytes());
    }
}

#[test]
fn write_response_daemon_error_returns_message_without_stdout() {
    // Arrange
    let response = Response::Error {
        kind: ErrorKind::PairingRefused,
        message: "pairing refused".into(),
    };
    let mut output = Vec::new();

    // Act
    let result = write_response(&mut output, &response);

    // Assert
    assert_eq!(result.unwrap_err().to_string(), "pairing refused");
    assert_eq!(output, [] as [u8; 0]);
}

#[test]
fn resolve_machine_id_names_and_prefixes_resolve_or_reject() {
    // Arrange
    let a = "abcd456789abcdef0123456789abcdef".parse().unwrap();
    let b = "abcd556789abcdef0123456789abcdef".parse().unwrap();
    let machines = [(a, "workstation"), (b, "laptop")];
    let cases = [
        ("workstation", Some(a)),
        ("ABCD4", Some(a)),
        ("abcd556789abcdef0123456789abcdef", Some(b)),
        ("abcd", None),
        ("abc", None),
        ("unknown", None),
    ];
    for (target, expected) in cases {
        // Act
        let result = resolve_machine_id(target, machines.into_iter());

        // Assert
        assert_eq!(result.ok(), expected);
    }
}

#[test]
fn poll_send_offer_states_map_to_progress_outcome_or_waiting() {
    // Arrange
    let peer = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let transfer = FileTransfer {
        id: 3,
        offer_id: Some(9),
        peer,
        direction: TransferDirection::Send,
        files_done: 0,
        files_total: 1,
        bytes_done: 0,
        bytes_total: 1,
    };
    let finished = |offer_id, outcome| FinishedSend {
        offer_id,
        peer,
        outcome,
    };
    let status = |transfers: Vec<FileTransfer>, finished_sends: Vec<FinishedSend>| StatusInfo {
        held_clipboard: None,
        transfers,
        held_files: Vec::new(),
        finished_sends,
        input: None,
        paused: false,
        focused: peer,
        pid: 0,
        machine: peer,
        name: "local".into(),
        peers: Vec::new(),
        nearby: Vec::new(),
        missing_permissions: Vec::new(),
        firewall: Vec::new(),
    };
    let cases = [
        (status(vec![], vec![]), files::Poll::Waiting),
        (
            status(vec![transfer.clone()], vec![]),
            files::Poll::Progress(&transfer),
        ),
        (
            status(vec![], vec![finished(9, SendOutcome::Failed)]),
            files::Poll::Finished(SendOutcome::Failed),
        ),
        (
            status(
                vec![transfer.clone()],
                vec![finished(8, SendOutcome::Completed)],
            ),
            files::Poll::Progress(&transfer),
        ),
    ];
    for (status, expected) in &cases {
        // Act
        let result = files::poll_send(status, 9);

        // Assert
        assert_eq!(&result, expected);
    }
}
