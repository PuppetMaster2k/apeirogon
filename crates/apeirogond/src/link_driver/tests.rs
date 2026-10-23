// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, RwLock};

use a7n_proto::v1::{InputBatch, Motion, PeerInfo};
use tokio::time::timeout;

use super::*;
use crate::identity::{self, Identity};
use crate::tls::{self, TrustSnapshot};
use crate::{endpoint, handshake};

fn endpoint(own: &Identity, peer: &Identity) -> quinn::Endpoint {
    let trust = Arc::new(RwLock::new(TrustSnapshot {
        pins: [peer.pin].into_iter().collect(),
        pairing_until: None,
    }));
    let mut endpoint = endpoint::bind_addr(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        tls::server_config(own, Arc::clone(&trust)).unwrap(),
    )
    .unwrap();
    endpoint.set_default_client_config(tls::client_config(own, trust).unwrap());
    endpoint
}

#[tokio::test]
async fn drive_motion_and_batches_uses_datagram_and_numbers_reliable_input() {
    // Arrange
    let dirs = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let own = identity::load_or_create(dirs[0].path()).unwrap();
    let peer = identity::load_or_create(dirs[1].path()).unwrap();
    let client = endpoint(&own, &peer);
    let server = endpoint(&peer, &own);
    let info = PeerInfo::default();
    let accept = async {
        let connection = server.accept().await.unwrap().await.unwrap();
        handshake::respond(connection, &info, true).await.unwrap()
    };
    let (link, mut remote) = tokio::join!(
        endpoint::dial(&client, server.local_addr().unwrap(), &info),
        accept,
    );
    let (outbound, mailbox) = mpsc::channel(4);
    let (inbound, mut received) = mpsc::channel(4);
    let task = tokio::spawn(drive(link.unwrap(), inbound, mailbox, mpsc::channel(4).0));
    let motion = Motion {
        route: 1,
        motion_seq: 1,
        x: 3,
        y: -2,
    };

    // Act
    let (datagram, batches, ack) = timeout(limits::HANDSHAKE_TIMEOUT, async {
        outbound
            .send(Envelope {
                body: Some(Body::Motion(motion)),
            })
            .await
            .unwrap();
        let datagram = remote.connection.read_datagram().await.unwrap();
        for _ in 0..2 {
            outbound
                .send(Envelope {
                    body: Some(Body::Input(InputBatch::default())),
                })
                .await
                .unwrap();
        }
        let mut batches = Vec::new();
        while batches.len() < 2 {
            let envelope = read_frame(
                &mut remote.control_recv,
                &mut remote.control_buf,
                FrameKind::Control,
            )
            .await
            .unwrap();
            if !matches!(envelope.body, Some(Body::Ping(_))) {
                batches.push(envelope);
            }
        }
        send_envelope(
            &mut remote.control_send,
            &mut Vec::new(),
            Body::Ack(Ack {
                seq: 2,
                held_us: 0,
                locked: true,
            }),
        )
        .await
        .unwrap();
        let (_, _, ack) = received.recv().await.unwrap();
        (datagram, batches, ack)
    })
    .await
    .unwrap();

    // Assert
    assert_eq!(decode_datagram(&datagram).unwrap(), Some(motion));
    for (envelope, seq) in batches.into_iter().zip(1..=2) {
        let Some(Body::Input(batch)) = envelope.body else {
            panic!("expected reliable input");
        };
        assert_eq!(batch.seq, seq);
    }
    assert!(matches!(
        ack.body,
        Some(Body::Ack(Ack { locked: true, .. }))
    ));
    drop(outbound);
    assert!(matches!(task.await.unwrap(), Err(LinkError::InboundClosed)));
}
