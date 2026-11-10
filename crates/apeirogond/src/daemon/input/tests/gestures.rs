// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{Capability, Focus as WireFocus, Gesture, Platform};

use super::*;

async fn receiver(capabilities: Vec<Capability>) -> (Daemon, TempDir, FakeInput, Daemon, TempDir) {
    let (mut owner, owner_dir, _) = daemon(false);
    let (mut receiver, dir, input) = daemon(true);
    owner
        .trust
        .write()
        .unwrap()
        .pins
        .insert(receiver.identity.pin);
    receiver
        .trust
        .write()
        .unwrap()
        .pins
        .insert(owner.identity.pin);
    let (outgoing, incoming) = tokio::join!(
        async {
            owner
                .endpoint
                .connect(
                    receiver.endpoint.local_addr().unwrap(),
                    crate::tls::TLS_SERVER_NAME,
                )
                .unwrap()
                .await
                .unwrap()
        },
        async { receiver.endpoint.accept().await.unwrap().await.unwrap() },
    );
    let (sender, _outbound) = mpsc::channel(1);
    let peer = owner.identity.machine_id;
    receiver.links.by_peer.insert(
        peer,
        crate::link::LinkHandle {
            platform: Platform::Macos,
            capabilities,
            sender,
            connection: incoming,
            role: crate::link::Role::Responder,
        },
    );
    let wire = WireFocus {
        route: 1,
        target: receiver.identity.machine_id.as_bytes().to_vec(),
        ..WireFocus::default()
    };
    receiver.input.focus.on_focus(peer, &wire, true, now());
    let (sender, _outbound) = mpsc::channel(1);
    owner.links.by_peer.insert(
        receiver.identity.machine_id,
        crate::link::LinkHandle {
            platform: Platform::Windows,
            capabilities: vec![],
            sender,
            connection: outgoing,
            role: crate::link::Role::Initiator,
        },
    );
    (receiver, dir, input.unwrap(), owner, owner_dir)
}

fn swipe(gesture: Gesture) -> InputEvent {
    InputEvent {
        event: Some(input_event::Event::Gesture(gesture.into())),
    }
}

#[tokio::test]
async fn receive_input_unnegotiated_gesture_is_dropped() {
    // Arrange
    let (mut daemon, _dir, mut input, owner, _owner_dir) = receiver(vec![]).await;
    let batch = InputBatch {
        route: 1,
        events: vec![swipe(Gesture::SwipeUp)],
        ..InputBatch::default()
    };

    // Act
    daemon
        .receive_input(owner.identity.machine_id, &batch)
        .await;

    // Assert
    assert!(input.inject.try_recv().is_err());
}

#[tokio::test]
async fn receive_input_multiple_gestures_in_batch_expands_only_first() {
    // Arrange
    let (mut daemon, _dir, mut input, owner, _owner_dir) =
        receiver(vec![Capability::Gestures]).await;
    let batch = InputBatch {
        route: 1,
        events: vec![
            swipe(Gesture::SwipeUp),
            key(true),
            swipe(Gesture::SwipeDown),
        ],
        ..InputBatch::default()
    };

    // Act
    daemon
        .receive_input(owner.identity.machine_id, &batch)
        .await;

    // Assert
    if a7n_platform::input::GESTURE_CHORDS_SUPPORTED {
        let expected: Vec<_> = daemon
            .config
            .gesture_bindings()
            .events(Gesture::SwipeUp)
            .collect();
        assert!(
            matches!(input.inject.try_recv().unwrap(), Inject::Chord(keys) if keys == expected)
        );
    } else {
        assert!(
            matches!(input.inject.try_recv().unwrap(), Inject::Event(event) if event == swipe(Gesture::SwipeUp))
        );
    }
    assert!(matches!(input.inject.try_recv().unwrap(), Inject::Event(event) if event == key(true)));
    assert!(input.inject.try_recv().is_err());
}

#[tokio::test]
async fn captured_swipe_while_driving_forwards_to_negotiated_peer() {
    // Arrange
    let (receiver, _dir, _input, mut owner, _owner_dir) = receiver(vec![]).await;
    let peer = receiver.identity.machine_id;
    let (sender, mut outbound) = mpsc::channel(1);
    let link = owner.links.by_peer.get_mut(&peer).unwrap();
    link.capabilities.push(Capability::Gestures);
    link.sender = sender;
    owner.take_peer(peer, None, now());
    owner
        .input
        .focus
        .on_accept(peer, &a7n_proto::v1::Accept { route: 1 });
    let event = swipe(Gesture::SwipeUp);

    // Act
    owner.captured(Some(Captured::Input(event.clone()))).await;

    // Assert
    assert!(matches!(outbound.try_recv().unwrap().body,
        Some(Body::Input(batch)) if batch.events == [event]));
    assert!(outbound.try_recv().is_err());
}
