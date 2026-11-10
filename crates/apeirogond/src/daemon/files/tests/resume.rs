// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{TransferOpen, TransferRequest, envelope::Body};

use super::*;
use crate::daemon::transfer::write;
use crate::pin_store::Entry;

async fn connect(a: &mut Daemon, b: &mut Daemon) {
    a.store_peer(Entry {
        machine_id: b.identity.machine_id,
        pin: b.identity.pin,
        name: "peer".into(),
        spki_der: None,
        hw_addr: None,
        screen_mm: None,
    })
    .unwrap();
    b.store_peer(Entry {
        machine_id: a.identity.machine_id,
        pin: a.identity.pin,
        name: "peer".into(),
        spki_der: None,
        hw_addr: None,
        screen_mm: None,
    })
    .unwrap();
    let accept = async {
        let connection = b.endpoint.accept().await.unwrap().await.unwrap();
        crate::handshake::respond(connection, &b.info, true)
            .await
            .unwrap()
    };
    let (outgoing, incoming) = tokio::join!(
        crate::endpoint::dial(&a.endpoint, b.endpoint.local_addr().unwrap(), &a.info),
        accept
    );
    a.register(outgoing.unwrap()).unwrap();
    b.register(incoming).unwrap();
}

async fn step(daemon: &mut Daemon) {
    tokio::select! {
        Some(event) = daemon.mailboxes.event_rx.recv() => daemon.event(event),
        Some((peer, link, envelope)) = daemon.mailboxes.inbound_rx.recv() => {
            if daemon.is_current(peer, link) { daemon.receive_from(peer, envelope).await.unwrap(); }
        },
        Some(stream) = daemon.mailboxes.transfer_rx.recv() => daemon.clipboard_stream(stream),
    }
}

async fn pump(a: &mut Daemon, b: &mut Daemon, condition: impl Fn(&Daemon, &Daemon) -> bool) {
    timeout(TEST_TIMEOUT, async {
        while !condition(a, b) {
            tokio::select! { () = step(a) => {}, () = step(b) => {}, () = tokio::task::yield_now() => {} }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn receive_files_link_loss_resumes_only_verified_prefix_remainder() {
    // Arrange
    let (mut a, a_dir) = daemon();
    let (mut b, _b_dir) = daemon();
    let peer = a.identity.machine_id;
    let sink = b.identity.machine_id;
    b.config
        .set(&format!("peers.{peer}.files"), "receive")
        .unwrap();
    let source = a_dir.path().join("source");
    let data = b"resumable payload";
    fs::write(&source, data).unwrap();
    let offer = Arc::new(offer::build(peer, 1, vec![source]).unwrap());
    a.files.offers.insert((sink, 1), offer.clone());
    connect(&mut a, &mut b).await;
    // Hold the first request at the source so this test can interrupt at an exact offset.
    let request = timeout(TEST_TIMEOUT, async {
        loop {
            tokio::select! {
                Some((_, _, envelope)) = a.mailboxes.inbound_rx.recv() => {
                    if let Some(Body::TransferRequest(request)) = envelope.body { break request; }
                },
                () = step(&mut b) => {},
            }
        }
    })
    .await
    .unwrap();
    let connection = a.links.by_peer[&sink].connection.clone();
    let mut stream = connection.open_uni().await.unwrap();
    let mut buf = Vec::new();
    write(
        &mut stream,
        &mut buf,
        Frame::Open(TransferOpen {
            transfer_id: request.transfer_id,
        }),
    )
    .await
    .unwrap();
    write(
        &mut stream,
        &mut buf,
        Frame::Chunk(FileChunk {
            file_index: 0,
            offset: 0,
            data: data[..5].to_vec(),
        }),
    )
    .await
    .unwrap();
    pump(&mut a, &mut b, |_, b| {
        b.file_transfers()
            .iter()
            .any(|progress| progress.bytes_done == 5)
    })
    .await;
    connection.close(0u32.into(), b"");
    a.links.by_peer.remove(&sink);
    a.input_link_lost(sink);
    b.links
        .by_peer
        .remove(&peer)
        .unwrap()
        .connection
        .close(0u32.into(), b"");
    b.input_link_lost(peer);
    pump(&mut a, &mut b, |_, b| b.files.active.is_empty()).await;
    let stage = b.files.incoming[&(peer, 1)].staging.clone();
    assert_eq!(fs::read(stage.join("source")).unwrap(), &data[..5]);

    // Act
    connect(&mut a, &mut b).await;
    let resumed: TransferRequest = timeout(TEST_TIMEOUT, async {
        loop {
            tokio::select! {
                Some((_, _, envelope)) = a.mailboxes.inbound_rx.recv() => {
                    if let Some(Body::TransferRequest(request)) = envelope.body { break request; }
                },
                () = step(&mut b) => {},
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(resumed.resume[0].offset, 5);
    assert!(a.files_request(sink, &resumed));
    let final_path = b.config.files.receive_dir().unwrap().join("source");
    pump(&mut a, &mut b, |_, b| b.files.incoming.is_empty()).await;

    // Assert
    assert_eq!(fs::read(final_path).unwrap(), data);
    assert!(!stage.exists());
}

#[tokio::test]
async fn accept_files_replaced_offer_returns_invalid_request_without_receiving() {
    // Arrange
    let (mut a, a_dir) = daemon();
    let (mut b, _b_dir) = daemon();
    let peer = a.identity.machine_id;
    let source = a_dir.path().join("source");
    fs::write(&source, b"payload").unwrap();
    connect(&mut a, &mut b).await;
    let first = offer::build(peer, 1, vec![source.clone()]).unwrap().wire;
    b.receive_file_offer(peer, &first);
    let seen = b.held_files().pop().unwrap();
    let newer = offer::build(peer, 2, vec![source]).unwrap().wire;
    b.receive_file_offer(peer, &newer);

    // Act
    let result = b.accept_files(seen.peer, seen.offer_id);

    // Assert
    assert!(matches!(
        result,
        Err(DaemonError::InvalidRequest(
            "file offer changed; review it again"
        ))
    ));
    assert_eq!(b.file_transfers(), []);
    assert!(!b.config.files.receive_dir().unwrap().exists());
    let held = b.held_files();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].offer_id, newer.generation);
}

#[tokio::test]
async fn files_request_clipboard_reference_with_same_generation_is_not_a_send() {
    // Arrange
    let (mut a, a_dir) = daemon();
    let (mut b, _b_dir) = daemon();
    let source = a_dir.path().join("source");
    fs::write(&source, b"payload").unwrap();
    let offer = Arc::new(offer::build(a.identity.machine_id, 1, vec![source]).unwrap());
    let peer = b.identity.machine_id;
    a.files.offers.insert((peer, 1), offer);
    connect(&mut a, &mut b).await;
    let request = TransferRequest {
        source: Some(a7n_proto::v1::transfer_request::Source::Clipboard(
            a7n_proto::v1::ClipboardRef {
                origin: a.identity.machine_id.as_bytes().to_vec(),
                generation: 1,
                send: false,
            },
        )),
        ..TransferRequest::default()
    };

    // Act
    let is_send = a.files_request(peer, &request);

    // Assert
    assert!(!is_send);
    assert!(a.files.active.is_empty());
}

#[tokio::test]
async fn receive_file_offer_shared_slots_queues_until_clipboard_slot_is_released() {
    // Arrange
    let (mut a, a_dir) = daemon();
    let (mut b, _b_dir) = daemon();
    let peer = a.identity.machine_id;
    b.config
        .set(&format!("peers.{peer}.files"), "receive")
        .unwrap();
    let source = a_dir.path().join("source");
    fs::write(&source, b"payload").unwrap();
    connect(&mut a, &mut b).await;
    let permit = b.clipboard.links[&peer]
        .slots
        .clone()
        .try_acquire_many_owned(
            u32::try_from(a7n_proto::limits::CONCURRENT_TRANSFERS_PER_LINK).unwrap(),
        )
        .unwrap();
    let wire = offer::build(peer, 1, vec![source]).unwrap().wire;
    b.config.set(&format!("peers.{peer}.files"), "ask").unwrap();
    b.receive_file_offer(peer, &wire);
    b.accept_files(peer, 1).unwrap();
    assert!(b.files.active.is_empty());
    assert_eq!(b.held_files(), []);
    b.tasks.spawn(async move {
        drop(permit);
    });

    // Act
    b.tasks.join_next().await.unwrap().unwrap();
    b.start_waiting_files(peer);

    // Assert
    assert_eq!(b.file_transfers().len(), 1);
    assert_eq!(
        b.clipboard.links[&peer].slots.available_permits(),
        a7n_proto::limits::CONCURRENT_TRANSFERS_PER_LINK - 1
    );
}

#[tokio::test]
async fn apply_config_file_policy_disabled_cancels_active_receive() {
    // Arrange
    let (mut a, a_dir) = daemon();
    let (mut b, _b_dir) = daemon();
    let peer = a.identity.machine_id;
    b.config
        .set(&format!("peers.{peer}.files"), "receive")
        .unwrap();
    let source = a_dir.path().join("source");
    fs::write(&source, b"payload").unwrap();
    connect(&mut a, &mut b).await;
    let wire = offer::build(peer, 1, vec![source]).unwrap().wire;
    b.receive_file_offer(peer, &wire);
    let stage = b.files.incoming[&(peer, 1)].staging.clone();
    let mut config = b.config.clone();
    config.set(&format!("peers.{peer}.files"), "off").unwrap();

    // Act
    b.apply_config(config).await;
    pump(&mut a, &mut b, |_, b| b.files.active.is_empty()).await;

    // Assert
    assert!(!stage.exists());
    assert!(b.files.incoming.is_empty());
}
