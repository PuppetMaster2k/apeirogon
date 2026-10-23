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
fn motion_grabbed_keeps_anchor_and_reports_each_swallowed_delta() {
    // Arrange
    let (events, _) = tokio::sync::mpsc::channel(1);
    let mut state = CaptureState {
        events,
        deadman: DeadMan::new(Instant::now()),
        grabbed: true,
        queue_full: false,
        last: POINT { x: 100, y: 200 },
        keys: [false; HELD_KEY_COUNT],
    };

    // Act
    let reports = [POINT { x: 103, y: 198 }, POINT { x: 104, y: 201 }].map(|p| state.motion(p));

    // Assert
    assert!(matches!(
        reports[0],
        Captured::Pointer {
            x: 100,
            y: 200,
            dx: 3,
            dy: -2
        }
    ));
    assert!(matches!(
        reports[1],
        Captured::Pointer {
            x: 100,
            y: 200,
            dx: 4,
            dy: 1
        }
    ));
}
