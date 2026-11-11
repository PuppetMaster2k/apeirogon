// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use tokio::sync::oneshot;

const CTRL: u32 = 0x0007_00e0;
const HOME: u32 = 0x0007_004a;

fn recording(
    kind: HotkeyKind,
    now: Instant,
) -> (Recording, oneshot::Receiver<Result<String, DaemonError>>) {
    let (reply, receiver) = oneshot::channel();
    (
        Recording {
            pending: Some(Pending {
                deadline: now + RECORD_TIMEOUT,
                kind,
                reply,
                keys: BTreeSet::new(),
            }),
            suppressed: BTreeSet::new(),
        },
        receiver,
    )
}

fn key(usage: u32, down: bool) -> KeyEvent {
    KeyEvent {
        usage,
        down,
        character: String::new(),
    }
}

#[test]
fn on_key_chord_waits_for_every_release_and_consumes_repeats() {
    // Arrange
    let (mut recording, mut reply) = recording(HotkeyKind::Chord, Instant::now());
    let events = [
        key(CTRL, true),
        key(HOME, true),
        key(CTRL, true),
        key(CTRL, false),
    ];
    for event in events {
        assert!(recording.on_key(&event));
    }
    assert!(matches!(
        reply.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));

    // Act
    let consumed = recording.on_key(&key(HOME, false));

    // Assert
    assert!(consumed);
    assert_eq!(reply.try_recv().unwrap().unwrap(), "ctrl+home");
    assert!(!recording.on_key(&key(HOME, true)));
}

#[test]
fn expire_held_key_finishes_and_suppresses_until_release() {
    // Arrange
    let now = Instant::now();
    let (mut recording, mut reply) = recording(HotkeyKind::Modifiers, now);
    assert!(recording.on_key(&key(CTRL, true)));

    // Act
    recording.expire(now + RECORD_TIMEOUT);

    // Assert
    assert_eq!(reply.try_recv().unwrap().unwrap(), "ctrl");
    assert!(recording.on_key(&key(CTRL, true)));
    assert!(recording.on_key(&key(CTRL, false)));
    assert!(!recording.on_key(&key(CTRL, true)));
    assert!(recording.next_deadline().is_none());
}

#[test]
fn expire_no_key_returns_invalid_request() {
    // Arrange
    let now = Instant::now();
    let (mut recording, mut reply) = recording(HotkeyKind::Trigger, now);

    // Act
    recording.expire(now + RECORD_TIMEOUT);

    // Assert
    assert!(matches!(
        reply.try_recv().unwrap(),
        Err(DaemonError::InvalidRequest("no key pressed"))
    ));
}

#[test]
fn on_key_invalid_trigger_consumes_every_key_until_release() {
    // Arrange
    let (mut recording, mut reply) = recording(HotkeyKind::Trigger, Instant::now());
    for event in [key(CTRL, true), key(HOME, true), key(HOME, false)] {
        assert!(recording.on_key(&event));
    }

    // Act
    let consumed = recording.on_key(&key(CTRL, false));

    // Assert
    assert!(consumed);
    assert!(matches!(
        reply.try_recv().unwrap(),
        Err(DaemonError::Hotkey(_))
    ));
}
