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
        keys: [false; HELD_KEY_COUNT],
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
fn tick_disconnected_control_stops_capture() {
    // Arrange
    let now = Instant::now();
    let (events, _receiver) = tokio::sync::mpsc::channel(1);
    let (sender, mut control) = tokio::sync::mpsc::channel(1);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(now),
        grabbed: true,
        absolute_motion: false,
        queue_full: false,
        last: POINT::default(),
        keys: [false; HELD_KEY_COUNT],
    };
    drop(sender);

    // Act
    let running = state.tick(&mut control, now);

    // Assert
    assert!(!running);
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
        keys: [false; HELD_KEY_COUNT],
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
fn mouse_own_warp_updates_baseline_without_reporting_local_activity() {
    // Arrange
    let (events, mut receiver) = tokio::sync::mpsc::channel(4);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(Instant::now()),
        grabbed: false,
        absolute_motion: false,
        queue_full: false,
        last: POINT::default(),
        keys: [false; HELD_KEY_COUNT],
    };
    let injected = MSLLHOOKSTRUCT {
        pt: POINT { x: 500, y: 100 },
        flags: LLMHF_INJECTED,
        dwExtraInfo: INJECTION_MARKER,
        ..Default::default()
    };

    // Act
    state.mouse(WM_MOUSEMOVE, &injected);
    state.mouse(
        WM_MOUSEMOVE,
        &MSLLHOOKSTRUCT {
            pt: POINT { x: 501, y: 102 },
            flags: 0,
            dwExtraInfo: 0,
            ..injected
        },
    );

    // Assert
    assert!(matches!(
        receiver.try_recv(),
        Ok(Captured::Pointer {
            x: 501,
            y: 102,
            dx: 1,
            dy: 2
        })
    ));
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
fn motion_absolute_fallback_keeps_anchor_and_reports_each_swallowed_delta() {
    // Arrange
    let (events, _) = tokio::sync::mpsc::channel(1);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(Instant::now()),
        grabbed: true,
        absolute_motion: true,
        queue_full: false,
        last: POINT { x: 100, y: 200 },
        keys: [false; HELD_KEY_COUNT],
    };

    // Act
    let reports = [POINT { x: 103, y: 198 }, POINT { x: 104, y: 201 }].map(|p| state.motion(p));

    // Assert
    assert!(matches!(
        reports[0],
        Some(Captured::Pointer {
            x: 100,
            y: 200,
            dx: 3,
            dy: -2
        })
    ));
    assert!(matches!(
        reports[1],
        Some(Captured::Pointer {
            x: 100,
            y: 200,
            dx: 4,
            dy: 1
        })
    ));
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
        (false, false, [true, false, false, false, false]),
        (false, true, [true, false, false, false, false]),
        (true, false, [false, true, false, false, false]),
        (true, true, [true, true, false, false, false]),
    ];

    for (grabbed, absolute, expected) in cases {
        // Act
        let deltas = sources.map(|source| select_delta(grabbed, absolute, source, (3, -2)));

        // Assert
        assert_eq!(deltas, expected.map(|selected| selected.then_some((3, -2))));
    }
}

#[test]
fn raw_motion_grabbed_emits_relative_deltas_and_ignores_synthetic_motion() {
    // Arrange
    let (events, mut receiver) = tokio::sync::mpsc::channel(4);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(Instant::now()),
        grabbed: true,
        absolute_motion: false,
        queue_full: false,
        last: POINT { x: 100, y: 200 },
        keys: [false; HELD_KEY_COUNT],
    };

    // Act
    state.raw_motion(true, false, 3, -2);
    state.raw_motion(false, true, 500, 100);
    let hook = state.motion(POINT { x: 110, y: 210 });

    // Assert
    assert!(matches!(
        receiver.try_recv(),
        Ok(Captured::Pointer {
            x: 100,
            y: 200,
            dx: 3,
            dy: -2,
        })
    ));
    assert!(receiver.try_recv().is_err());
    assert!(hook.is_none());
}
