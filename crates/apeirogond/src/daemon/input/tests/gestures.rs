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
async fn receive_input_accepted_swipe_injects_one_chord_in_order() {
    // Arrange
    let (mut daemon, dir, mut input, owner, _owner_dir) =
        receiver(vec![Capability::Gestures]).await;
    let path = dir.path().join("config.toml");
    daemon = daemon.with_config(a7n_ipc::config::Config::default(), path.clone());
    let mut changed = daemon.config.clone();
    changed.set("gestures.swipe_up", "ctrl+meta+left").unwrap();
    changed.save(&path).unwrap();
    daemon.reload_config().await.unwrap();
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
    if a7n_platform::input::GESTURE_CHORDS_SUPPORTED {
        let Inject::Chord(keys) = input.inject.try_recv().unwrap() else {
            panic!("expected one chord");
        };
        let events: Vec<_> = keys.iter().map(|key| (key.usage, key.down)).collect();
        assert_eq!(
            events,
            [
                (0x0007_00e0, true),
                (0x0007_00e3, true),
                (0x0007_0050, true),
                (0x0007_0050, false),
                (0x0007_00e3, false),
                (0x0007_00e0, false)
            ]
        );
    } else {
        assert!(
            matches!(input.inject.try_recv().unwrap(), Inject::Event(event) if event == swipe(Gesture::SwipeUp))
        );
    }
    assert!(input.inject.try_recv().is_err());
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
async fn forward_capture_swipe_requires_only_negotiated_capability() {
    // Arrange
    for (platform, capability, forwarded) in [
        (Platform::Windows, true, true),
        (Platform::Windows, false, false),
        (Platform::Linux, true, true),
        (Platform::Macos, true, true),
    ] {
        let (a, b, mut a_input, mut b_input) = configured_pair(|a, b| {
            a.info.platform = Platform::Macos.into();
            b.info.platform = platform.into();
            a.info.capabilities.push(Capability::Gestures.into());
            b.info
                .capabilities
                .retain(|value| *value != i32::from(Capability::Gestures));
            if capability {
                b.info.capabilities.push(Capability::Gestures.into());
            }
        })
        .await;
        drive(&mut a_input, &mut b_input).await;
        let swipe = InputEvent {
            event: Some(input_event::Event::Gesture(Gesture::SwipeRight.into())),
        };

        // Act
        a_input
            .capture
            .send(Captured::Input(swipe.clone()))
            .await
            .unwrap();
        a_input
            .capture
            .send(Captured::Input(key(true)))
            .await
            .unwrap();

        // Assert
        if forwarded {
            if a7n_platform::input::GESTURE_CHORDS_SUPPORTED {
                let expected: Vec<_> = a7n_ipc::config::Config::default()
                    .gesture_bindings()
                    .events(Gesture::SwipeRight)
                    .collect();
                assert!(
                    matches!(b_input.injected().await, Inject::Chord(keys) if keys == expected)
                );
            } else {
                assert!(matches!(b_input.injected().await, Inject::Event(event) if event == swipe));
            }
        }
        assert!(matches!(b_input.injected().await, Inject::Event(event) if event == key(true)));
        a.stop().await;
        b.stop().await;
    }
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

#[tokio::test]
async fn captured_swipe_at_home_does_not_forward_to_negotiated_peer() {
    // Arrange
    let (receiver, _dir, _input, mut owner, _owner_dir) = receiver(vec![]).await;
    let peer = receiver.identity.machine_id;
    let (sender, mut outbound) = mpsc::channel(1);
    let link = owner.links.by_peer.get_mut(&peer).unwrap();
    link.capabilities.push(Capability::Gestures);
    link.sender = sender;

    // Act
    owner
        .captured(Some(Captured::Input(swipe(Gesture::SwipeUp))))
        .await;

    // Assert
    assert_eq!(owner.input.focus.cursor(), Cursor::Home);
    assert!(outbound.try_recv().is_err());
}
