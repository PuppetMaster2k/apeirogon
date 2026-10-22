// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn parse_announcement_lan_records_filters_invalid_and_prefers_ipv4() {
    // Arrange
    let own = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    let peer = MachineId::try_from(&[2; MachineId::LEN][..]).unwrap();
    let own_hex = own.to_string();
    let peer_hex = peer.to_string();
    let version = PROTOCOL_VERSION.to_string();
    let v4: IpAddr = "192.168.1.2".parse().unwrap();
    let v6: IpAddr = "fd00::2".parse().unwrap();
    let link_local: IpAddr = "fe80::2".parse().unwrap();
    let port = a7n_proto::limits::DEFAULT_UDP_PORT;
    let cases = [
        (
            Some(peer_hex.as_str()),
            Some(version.as_str()),
            vec![v4],
            port,
            vec![v4],
        ),
        (
            Some(own_hex.as_str()),
            Some(version.as_str()),
            vec![v4],
            port,
            vec![],
        ),
        (Some(peer_hex.as_str()), Some("999"), vec![v4], port, vec![]),
        (
            Some("invalid"),
            Some(version.as_str()),
            vec![v4],
            port,
            vec![],
        ),
        (
            Some("éééééééééééééééé"),
            Some(version.as_str()),
            vec![v4],
            port,
            vec![],
        ),
        (None, Some(version.as_str()), vec![v4], port, vec![]),
        (Some(peer_hex.as_str()), None, vec![v4], port, vec![]),
        (
            Some(peer_hex.as_str()),
            Some(version.as_str()),
            vec![link_local],
            port,
            vec![],
        ),
        (
            Some(peer_hex.as_str()),
            Some(version.as_str()),
            vec![v6, link_local, v4, v4],
            port,
            vec![v4, v6],
        ),
        (
            Some(peer_hex.as_str()),
            Some(version.as_str()),
            vec![v4],
            0,
            vec![],
        ),
    ];

    for (id, version, addresses, port, expected) in cases {
        // Act
        let result = parse_announcement(own, id, version, addresses, port);

        // Assert
        let expected = (!expected.is_empty()).then(|| Announcement {
            id: peer,
            addrs: expected
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect(),
        });
        assert_eq!(result, expected, "id={id:?} version={version:?}");
    }
}

#[test]
fn advertisement_identity_and_listen_port_uses_only_protocol_txt() {
    // Arrange
    let own = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    let hex = own.to_string();
    let port = a7n_proto::limits::DEFAULT_UDP_PORT + 1;

    // Act
    let service = advertisement(own, port).unwrap();

    // Assert
    assert_eq!(
        service.get_fullname(),
        &format!("{hex}.{MDNS_SERVICE_TYPE}")
    );
    assert_eq!(service.get_hostname(), format!("{hex}.local."));
    assert_eq!(service.get_port(), port);
    assert_eq!(service.get_properties().len(), 2);
    assert_eq!(
        service.get_property_val_str(MDNS_ID_KEY),
        Some(hex.as_str())
    );
    assert_eq!(
        service.get_property_val_str(MDNS_VERSION_KEY),
        Some(PROTOCOL_VERSION.to_string().as_str())
    );
    assert!(service.is_addr_auto());
}
