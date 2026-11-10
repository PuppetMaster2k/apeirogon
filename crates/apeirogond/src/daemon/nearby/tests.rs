// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use a7n_proto::MachineId;

use crate::daemon::{Daemon, Entry};
use crate::discovery::{Announcement, Event as DiscoveryEvent, MAX_NEARBY};

#[tokio::test]
async fn discovery_unpaired_peer_lists_address_without_dialing() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let id = MachineId::try_from(&[2; MachineId::LEN][..]).unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, a7n_proto::limits::DEFAULT_UDP_PORT));

    // Act
    daemon.discovery(DiscoveryEvent::Discovered(Announcement {
        id,
        addrs: vec![addr],
    }));

    // Assert
    assert_eq!(
        daemon.status().nearby,
        vec![a7n_ipc::NearbyMachine { id, addr }]
    );
    assert!(daemon.nearby.discovery_dials.is_empty());
    assert!(daemon.tasks.is_empty());
    assert!(daemon.links.by_peer.is_empty());
}

#[tokio::test]
async fn discovery_full_unpaired_table_accepts_paired_and_preserves_lost_addresses() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let paired = MachineId::try_from(&[255; MachineId::LEN][..]).unwrap();
    daemon.pins.add(Entry {
        machine_id: paired,
        pin: daemon.identity.pin,
        name: "paired".into(),
        spki_der: None,
        hw_addr: None,
        screen_mm: None,
    });
    // Keep the test in-process without starting a dial.
    daemon.nearby.discovery_dials.insert(paired);
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, a7n_proto::limits::DEFAULT_UDP_PORT));
    for seed in 0..MAX_NEARBY {
        let mut bytes = [0; MachineId::LEN];
        bytes[..size_of::<usize>()].copy_from_slice(&seed.to_be_bytes());
        daemon
            .nearby
            .nearby
            .insert(MachineId::try_from(bytes.as_slice()).unwrap(), vec![addr]);
    }
    let fresh = SocketAddr::from((Ipv6Addr::LOCALHOST, addr.port()));

    // Act
    daemon.discovery(DiscoveryEvent::Discovered(Announcement {
        id: paired,
        addrs: vec![addr],
    }));
    let accepted = daemon.nearby.nearby.len();
    daemon.discovery(DiscoveryEvent::Discovered(Announcement {
        id: paired,
        addrs: vec![fresh, fresh],
    }));
    daemon.discovery(DiscoveryEvent::Lost(paired));

    // Assert
    assert_eq!(accepted, MAX_NEARBY);
    assert_eq!(daemon.nearby.nearby[&paired], vec![fresh, addr]);
}
