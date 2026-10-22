// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::RwLock;

use super::*;
use crate::daemon::test_peer::Peer;
use crate::identity;
use crate::tls::{self, TrustSnapshot};

/// QUIC TLS errors are 0x100 + alert; `bad_certificate` is alert 42.
const BAD_CERTIFICATE_CLOSE: u64 = 0x100 + 42;

#[tokio::test]
async fn dial_unknown_pin_outside_pairing_window_receives_bad_certificate() {
    // Arrange
    let host = Peer::start();
    let host_identity = identity::load_or_create(host.dir.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let identity = identity::load_or_create(dir.path()).unwrap();
    let trust = Arc::new(RwLock::new(TrustSnapshot {
        pins: [host_identity.pin].into_iter().collect(),
        pairing_until: None,
    }));
    let mut endpoint = bind_addr(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        tls::server_config(&identity, Arc::clone(&trust)).unwrap(),
    )
    .unwrap();
    endpoint.set_default_client_config(tls::client_config(&identity, trust).unwrap());

    // Act
    let result = dial(&endpoint, host.addr, &PeerInfo::default()).await;

    // Assert
    let Err(LinkError::Connection(lost) | LinkError::Read(quinn::ReadError::ConnectionLost(lost))) =
        result
    else {
        panic!("expected connection loss");
    };
    let quinn::ConnectionError::ConnectionClosed(close) = lost else {
        panic!("expected TLS close");
    };
    assert_eq!(u64::from(close.error_code), BAD_CERTIFICATE_CLOSE);
    assert!(host.status().await.peers.iter().all(|peer| !peer.connected));
    host.stop().await;
}
