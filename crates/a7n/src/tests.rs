// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{ErrorKind, PeerInfo, StatusInfo};

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
        machine,
        name: "local".into(),
        peers: [true, false]
            .into_iter()
            .map(|connected| PeerInfo {
                id: machine,
                name: "peer".into(),
                connected,
                rtt_ms: None,
            })
            .collect(),
        nearby: Vec::new(),
    });
    let mut output = Vec::new();

    // Act
    write_response(&mut output, &response).unwrap();

    // Assert
    assert_eq!(
        String::from_utf8(output).unwrap(),
        concat!(
            "local 0123456789abcdef0123456789abcdef (this machine)\n",
            "peer 0123456789abcdef0123456789abcdef connected\n",
            "peer 0123456789abcdef0123456789abcdef offline\n",
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
        machine,
        name: "local".into(),
        peers: Vec::new(),
        nearby: vec![NearbyMachine {
            id: machine,
            addr: "127.0.0.1:1234".parse().unwrap(),
        }],
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
