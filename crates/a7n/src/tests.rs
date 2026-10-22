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
fn parse_machine_hex_input_validates_and_preserves_bytes() {
    // Arrange
    let cases = [
        ("0123456789abcdef0123456789ABCDEF", true),
        ("00000000000000000000000000000000", true),
        ("", false),
        ("0123456789abcdef", false),
        ("+123456789abcdef0123456789abcdef", false),
        ("g123456789abcdef0123456789abcdef", false),
        ("é23456789abcdef0123456789abcdef", false),
        ("0123456789abcdef0123456789abcdef00", false),
    ];
    for (input, valid) in cases {
        // Act
        let result = parse_machine(input);

        // Assert
        assert_eq!(result.is_ok(), valid, "{input}");
        if let Ok(machine) = result {
            let mut output = Vec::new();
            write_machine(&mut output, machine).unwrap();
            assert_eq!(output, input.to_ascii_lowercase().as_bytes());
        }
    }
}

#[test]
fn write_response_status_displays_identity_and_peer_states() {
    // Arrange
    let machine = parse_machine("0123456789abcdef0123456789abcdef").unwrap();
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
                machine: parse_machine("0123456789abcdef0123456789abcdef").unwrap(),
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
