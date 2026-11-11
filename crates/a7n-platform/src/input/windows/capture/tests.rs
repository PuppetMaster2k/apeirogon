// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn tick_full_queue_and_expired_keepalive_releases_grab() {
    // Arrange
    let now = Instant::now();
    let (events, _receiver) = tokio::sync::mpsc::channel(1);
    let (_sender, mut control) = tokio::sync::mpsc::channel(1);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(now),
        grabbed: true,
        absolute_motion: false,
        queue_full: false,
        last: POINT { x: 100, y: 200 },
        keys: [LocalKey::default(); HELD_KEY_COUNT],
    };
    state.emit(Captured::Pointer {
        x: 100,
        y: 200,
        dx: 1,
        dy: 0,
    });
    state.emit(Captured::Pointer {
        x: 100,
        y: 200,
        dx: 2,
        dy: 0,
    });

    // Act
    let running = state.tick(&mut control, now + crate::input::DEADMAN_TIMEOUT);

    // Assert
    assert!(running);
    assert!(!state.grabbed);
}

#[test]
fn key_repeated_down_is_dropped_but_release_and_other_injection_are_captured() {
    // Arrange
    let (events, mut receiver) = tokio::sync::mpsc::channel(4);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(Instant::now()),
        grabbed: true,
        absolute_motion: false,
        queue_full: false,
        last: POINT::default(),
        keys: [LocalKey::default(); HELD_KEY_COUNT],
    };
    let key = KBDLLHOOKSTRUCT {
        vkCode: 0x41,
        scanCode: 0x1E,
        flags: LLKHF_INJECTED,
        time: 0,
        dwExtraInfo: 0,
    };
    state.key(&key);

    // Act
    state.key(&key);
    state.key(&KBDLLHOOKSTRUCT {
        flags: LLKHF_INJECTED | LLKHF_UP,
        ..key
    });

    // Assert
    for down in [true, false] {
        assert!(
            matches!(receiver.try_recv(), Ok(Captured::Input(InputEvent {
            event: Some(input_event::Event::Key(key))
        })) if key.usage == 0x0007_0004 && key.down == down)
        );
    }
    assert!(receiver.try_recv().is_err());
}

#[test]
fn own_injection_only_our_injected_marker_is_ignored() {
    // Arrange
    let cases = [
        (true, INJECTION_MARKER),
        (false, INJECTION_MARKER),
        (true, 0),
    ];

    // Act
    let ignored = cases.map(|(injected, marker)| own_injection(injected, marker));

    // Assert
    assert_eq!(ignored, [true, false, false]);
}

#[test]
fn select_delta_capture_modes_choose_one_motion_source() {
    // Arrange
    let sources = [
        MotionSource::Hook,
        MotionSource::Raw {
            has_device: true,
            absolute: false,
        },
        MotionSource::Raw {
            has_device: true,
            absolute: true,
        },
        MotionSource::Raw {
            has_device: false,
            absolute: false,
        },
        MotionSource::Raw {
            has_device: false,
            absolute: true,
        },
    ];
    let cases = [
        (false, [false, true, false, false, false]),
        (true, [true, true, false, false, false]),
    ];

    for (absolute, expected) in cases {
        // Act
        let deltas = sources.map(|source| select_delta(absolute, source, (3, -2)));

        // Assert
        assert_eq!(deltas, expected.map(|selected| selected.then_some((3, -2))));
    }
}

#[test]
fn key_down_before_grab_release_passes_through() {
    // Arrange
    let (events, mut receiver) = tokio::sync::mpsc::channel(2);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(Instant::now()),
        grabbed: false,
        absolute_motion: false,
        queue_full: false,
        last: POINT::default(),
        keys: [LocalKey::default(); HELD_KEY_COUNT],
    };
    let key = KBDLLHOOKSTRUCT {
        scanCode: 0x1d, // Left Ctrl.
        ..KBDLLHOOKSTRUCT::default()
    };
    assert!(!state.key(&key));
    state.grabbed = true;

    // Act
    let swallowed = state.key(&KBDLLHOOKSTRUCT {
        flags: LLKHF_UP,
        ..key
    });

    // Assert
    assert!(!swallowed);
    for down in [true, false] {
        assert!(
            matches!(receiver.try_recv(), Ok(Captured::Input(InputEvent {
            event: Some(input_event::Event::Key(key))
        })) if key.down == down)
        );
    }
    assert!(state.key(&key));
}
