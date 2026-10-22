// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use a7n_proto::MachineId;

use crate::daemon::test_peer::{Peer, linked};
use crate::daemon::{Daemon, Entry};
use crate::discovery::{Announcement, Event as DiscoveryEvent, MAX_NEARBY};

#[tokio::test]
async fn discovery_paired_unlinked_peers_falls_back_and_links_both_sides() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let mut a = Daemon::load(a_dir.path(), addr).unwrap();
    let mut b = Daemon::load(b_dir.path(), addr).unwrap();
    let a_identity = (a.identity.machine_id, a.identity.pin);
    let b_identity = (b.identity.machine_id, b.identity.pin);
    for (local, (machine_id, pin)) in [(&mut a, b_identity), (&mut b, a_identity)] {
        local.pins.add(Entry {
            machine_id,
            pin,
            name: "peer".into(),
            spki_der: None,
        });
        local.sync_trust().unwrap();
    }
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));

    // Act
    for (local, remote) in [(&a, &b), (&b, &a)] {
        local
            .discoveries
            .send(DiscoveryEvent::Discovered(Announcement {
                id: remote.machine,
                // An IPv4 endpoint rejects IPv6 immediately, then tries the usable address.
                addrs: vec![
                    SocketAddr::from((Ipv6Addr::LOCALHOST, remote.addr.port())),
                    remote.addr,
                ],
            }))
            .await
            .unwrap();
    }

    // Assert
    for (local, remote) in [(&a, &b), (&b, &a)] {
        let status = local.wait(|status| linked(status, remote.machine)).await;
        assert_eq!(status.nearby, []);
        assert_eq!(status.peers.iter().filter(|peer| peer.connected).count(), 1);
    }
    a.stop().await;
    b.stop().await;
}

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
    assert!(daemon.discovery_dials.is_empty());
    assert!(daemon.tasks.is_empty());
    assert!(daemon.links.by_peer.is_empty());
}

#[tokio::test]
async fn discovery_full_registry_ignores_new_ids_and_forgets_lost_peers() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, a7n_proto::limits::DEFAULT_UDP_PORT));
    for seed in 0..MAX_NEARBY {
        let mut bytes = [0; MachineId::LEN];
        bytes[..size_of::<usize>()].copy_from_slice(&seed.to_be_bytes());
        let id = MachineId::try_from(bytes.as_slice()).unwrap();
        daemon.discovery(DiscoveryEvent::Discovered(Announcement {
            id,
            addrs: vec![addr],
        }));
    }
    let extra = MachineId::try_from(&[255; MachineId::LEN][..]).unwrap();
    let existing = *daemon.nearby.keys().next().unwrap();

    // Act
    daemon.discovery(DiscoveryEvent::Discovered(Announcement {
        id: extra,
        addrs: vec![addr],
    }));
    daemon.discovery(DiscoveryEvent::Lost(existing));

    // Assert
    assert_eq!(daemon.nearby.len(), MAX_NEARBY - 1);
    assert!(!daemon.nearby.contains_key(&existing));
    assert!(!daemon.nearby.contains_key(&extra));
}
