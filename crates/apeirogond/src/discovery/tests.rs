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
fn service_event_full_name_table_forwards_unknown_service_without_growing() {
    // Arrange
    let own = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    let peer = MachineId::try_from(&[2; MachineId::LEN][..]).unwrap();
    let mut names: HashMap<_, _> = (0..MAX_NEARBY)
        .map(|index| (format!("service-{index}.{MDNS_SERVICE_TYPE}"), own))
        .collect();
    let port = a7n_proto::limits::DEFAULT_UDP_PORT;
    let address: IpAddr = "192.168.1.2".parse().unwrap();
    let mut service = advertisement(peer, port).unwrap().as_resolved_service();
    service.addresses.insert(address.into());
    let fullname = service.fullname.clone();

    // Act
    let forwarded = service_event(
        own,
        ServiceEvent::ServiceResolved(Box::new(service)),
        &mut names,
    );
    let removed = service_event(
        own,
        ServiceEvent::ServiceRemoved(MDNS_SERVICE_TYPE.into(), fullname.clone()),
        &mut names,
    );

    // Assert
    assert!(matches!(forwarded, Some(Event::Discovered(announcement))
        if announcement == Announcement { id: peer, addrs: vec![SocketAddr::new(address, port)] }));
    assert_eq!(names.len(), MAX_NEARBY);
    assert!(!names.contains_key(&fullname));
    assert!(removed.is_none());
}
