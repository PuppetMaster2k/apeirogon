// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use a7n_core::layout::Placement;
use a7n_proto::v1::{InputEvent, KeyEvent, RefuseReason, input_event};
use a7n_proto::{FrameKind, limits};
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::*;
use crate::daemon::test_peer::{Peer, TEST_TIMEOUT};
use crate::pin_store::Entry;

mod cursor;
mod feedback;
mod fullscreen;
mod gestures;
mod hotkeys;
mod local_keys;
mod pause;

struct FakeInput {
    capture: mpsc::Sender<Captured>,
    control: mpsc::Receiver<CaptureControl>,
    inject: mpsc::Receiver<Inject>,
    _feedback: mpsc::Sender<Captured>,
}

fn daemon(input: bool) -> (Daemon, TempDir, Option<FakeInput>) {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    if !input {
        return (daemon, dir, None);
    }
    let (handles, fake) = fake_input();
    (daemon.with_input(Some(handles)), dir, Some(fake))
}

fn fake_input() -> (Handles, FakeInput) {
    let (capture_tx, capture_rx) = mpsc::channel(super::super::MAILBOX_CAPACITY);
    let (control_tx, control_rx) = mpsc::channel(super::super::MAILBOX_CAPACITY);
    let (inject_tx, inject_rx) = mpsc::channel(super::super::MAILBOX_CAPACITY);
    let (feedback_tx, feedback_rx) = mpsc::channel(super::super::MAILBOX_CAPACITY);
    let bounds = Bounds {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    };
    (
        (
            Capture {
                bounds,
                monitors: vec![bounds].into(),
                events: capture_rx,
                control: control_tx,
            },
            Injector {
                bounds,
                monitors: vec![bounds].into(),
                commands: inject_tx,
                pointer: feedback_rx,
            },
        ),
        FakeInput {
            capture: capture_tx,
            control: control_rx,
            inject: inject_rx,
            _feedback: feedback_tx,
        },
    )
}

async fn pair() -> (Peer, Peer, FakeInput, FakeInput) {
    configured_pair(|_, _| {}).await
}

async fn configured_pair(
    configure: impl FnOnce(&mut Daemon, &mut Daemon),
) -> (Peer, Peer, FakeInput, FakeInput) {
    let (mut a, a_dir, a_input) = daemon(true);
    let (mut b, b_dir, b_input) = daemon(true);
    configure(&mut a, &mut b);
    let placements = [a.identity.machine_id, b.identity.machine_id]
        .into_iter()
        .zip([0, 100])
        .map(|(id, x)| Placement {
            id,
            x,
            y: 0,
            width: 100,
            height: 100,
            blocked: 0,
        })
        .collect();
    a.layout.state = a
        .layout
        .state
        .edit(placements, a.identity.machine_id)
        .unwrap();
    b.layout.state = a.layout.state.clone();
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    (a, b, a_input.unwrap(), b_input.unwrap())
}

impl FakeInput {
    async fn grab(&mut self, expected: bool) {
        timeout(TEST_TIMEOUT, async {
            loop {
                if matches!(self.control.recv().await.unwrap(), CaptureControl::Grab(value) if value == expected) { return; }
            }
        }).await.unwrap();
    }

    async fn injected(&mut self) -> Inject {
        timeout(TEST_TIMEOUT, self.inject.recv())
            .await
            .unwrap()
            .unwrap()
    }

    async fn cross(&self, right: bool) {
        self.capture.send(pointer(right)).await.unwrap();
    }
}

fn pointer(right: bool) -> Captured {
    let resistance = i32::try_from(limits::DEFAULT_RESISTANCE_PX).unwrap();
    Captured::Pointer {
        x: if right { 99 } else { 0 },
        y: 50,
        dx: if right { resistance } else { -resistance },
        dy: 0,
    }
}

fn key(down: bool) -> InputEvent {
    InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage: 0x0007_0004,
            down,
            character: String::new(),
        })),
    }
}

#[tokio::test]
async fn captured_right_edge_takes_peer_grabs_and_warps() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = pair().await;

    // Act
    a_input.cross(true).await;

    // Assert
    a_input.grab(true).await;
    b_input.grab(true).await;
    assert!(matches!(
        b_input.injected().await,
        Inject::Warp { x: 0, y: 0..=99 }
    ));
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn receive_focus_unavailable_or_denied_refuses_not_allowed() {
    for backend in [false, true] {
        // Arrange
        let (a, _a_dir, _) = daemon(false);
        let (mut b, b_dir, _input) = daemon(backend);
        if backend {
            b.config
                .set(
                    &format!("peers.{}.allow_drive", a.identity.machine_id),
                    "false",
                )
                .unwrap();
        }
        b.pins.add(Entry {
            machine_id: a.identity.machine_id,
            pin: a.identity.pin,
            name: "owner".into(),
            spki_der: None,
            hw_addr: None,
            screen_mm: None,
        });
        b.sync_trust().unwrap();
        // Trust the responder for A's outgoing TLS handshake.
        a.trust.write().unwrap().pins.insert(b.identity.pin);
        let b = Peer::run(b, b_dir);
        let connection = timeout(
            limits::HANDSHAKE_TIMEOUT,
            a.endpoint
                .connect(b.addr, crate::tls::TLS_SERVER_NAME)
                .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        let mut buffer = Vec::new();
        let mut incoming = Vec::new();

        // Act
        crate::handshake::send_envelope(
            &mut send,
            &mut buffer,
            Body::Hello(a7n_proto::v1::Hello {
                protocol_version: a7n_proto::PROTOCOL_VERSION,
                info: Some(a.info.clone()),
            }),
        )
        .await
        .unwrap();
        let welcome = timeout(
            TEST_TIMEOUT,
            crate::handshake::read_frame(&mut recv, &mut incoming, FrameKind::Control),
        )
        .await
        .unwrap()
        .unwrap();
        crate::handshake::send_envelope(
            &mut send,
            &mut buffer,
            Body::Focus(a7n_proto::v1::Focus {
                route: 1,
                target: b.machine.as_bytes().to_vec(),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let refusal = timeout(TEST_TIMEOUT, async {
            loop {
                let envelope =
                    crate::handshake::read_frame(&mut recv, &mut incoming, FrameKind::Control)
                        .await
                        .unwrap();
                if let Some(Body::Refuse(refuse)) = envelope.body {
                    break refuse;
                }
            }
        })
        .await
        .unwrap();

        // Assert
        assert!(matches!(welcome.body, Some(Body::Welcome(welcome)) if !welcome.accepts_drive));
        assert_eq!(refusal.route, 1);
        assert_eq!(refusal.reason, i32::from(RefuseReason::NotAllowed));
        b.stop().await;
    }
}

#[tokio::test]
async fn receive_input_full_injector_waits_and_delivers_both_events() {
    // Arrange
    let (mut daemon, _dir, input) = daemon(true);
    let mut input = input.unwrap();
    let (commands, receiver) = mpsc::channel(1);
    daemon.input.injector.as_mut().unwrap().commands = commands;
    input.inject = receiver;
    let peer = MachineId::try_from(&[42; MachineId::LEN][..]).unwrap();
    daemon.pins.add(Entry {
        machine_id: peer,
        pin: daemon.identity.pin,
        name: "owner".into(),
        spki_der: None,
        hw_addr: None,
        screen_mm: None,
    });
    daemon
        .receive_from(
            peer,
            Envelope {
                body: Some(Body::Focus(a7n_proto::v1::Focus {
                    route: 1,
                    target: daemon.identity.machine_id.as_bytes().to_vec(),
                    ..Default::default()
                })),
            },
        )
        .await
        .unwrap();

    // Act
    let task = tokio::spawn(async move {
        daemon
            .receive_from(
                peer,
                Envelope {
                    body: Some(Body::Input(InputBatch {
                        route: 1,
                        events: vec![key(true), key(false)],
                        ..Default::default()
                    })),
                },
            )
            .await
            .unwrap();
        daemon
    });
    timeout(TEST_TIMEOUT, async {
        while input.inject.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let blocked = !task.is_finished();
    let queued = input.inject.len();
    let first = input.injected().await;
    let second = input.injected().await;
    let daemon = timeout(TEST_TIMEOUT, task).await.unwrap().unwrap();

    // Assert
    assert!(blocked, "dispatch must wait for backend capacity");
    assert_eq!(queued, 1);
    assert!(matches!(first, Inject::Event(event) if event == key(true)));
    assert!(matches!(second, Inject::Event(event) if event == key(false)));
    assert!(!daemon.input.failed);
}

#[tokio::test]
async fn finish_input_retry_after_backend_closes_restores_input() {
    // Arrange
    let (mut daemon, _dir, _old_input) = daemon(true);
    daemon.input.closed();
    let (handles, mut restarted) = fake_input();

    // Act
    daemon.finish_input_retry(Some(Ok(handles)));
    daemon.input.grab(true).await;

    // Assert
    assert!(!daemon.input.failed);
    assert!(daemon.status().input.is_none());
    restarted.grab(true).await;
}

#[tokio::test]
async fn captured_offline_edge_waits_for_resistance_before_waking() {
    // Arrange
    let (mut daemon, _dir, _input) = daemon(true);
    let peer = MachineId::try_from(&[42; MachineId::LEN][..]).unwrap();
    daemon.pins.add(Entry {
        machine_id: peer,
        pin: daemon.identity.pin,
        name: "desktop".into(),
        spki_der: None,
        hw_addr: Some([2; 6]),
        screen_mm: None,
    });
    daemon.layout.state = daemon
        .layout
        .state
        .edit(
            [daemon.identity.machine_id, peer]
                .into_iter()
                .zip([0, 100])
                .map(|(id, x)| Placement {
                    id,
                    x,
                    y: 0,
                    width: 100,
                    height: 100,
                    blocked: 0,
                })
                .collect(),
            daemon.identity.machine_id,
        )
        .unwrap();
    let (packets, mut received) = mpsc::channel(1);
    daemon.wol.sender = Box::new(move |mac| {
        packets.try_send(mac).unwrap();
        Ok(())
    });
    daemon
        .captured(Some(Captured::Pointer {
            x: 99,
            y: 50,
            dx: 1,
            dy: 0,
        }))
        .await;
    assert!(received.try_recv().is_err());

    // Act
    daemon.captured(Some(pointer(true))).await;

    // Assert
    assert_eq!(received.try_recv().unwrap(), [2; 6]);
    assert!(daemon.status().peers[0].waking);
    assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
}
