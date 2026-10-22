// SPDX-License-Identifier: GPL-3.0-or-later

//! Needs an X server: `xvfb-run -a cargo test -p a7n-platform -- --ignored`.
#![cfg(all(unix, not(target_os = "macos")))]

use std::thread::sleep;
use std::time::{Duration, Instant};

use a7n_platform::input::{CaptureControl, Captured, DEADMAN_TIMEOUT, Inject, start};
use a7n_proto::v1::{InputEvent, KeyEvent, input_event};
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, GrabStatus, Window};
use x11rb::rust_connection::RustConnection;

const KEY_A_USAGE: u32 = 0x0007_0004;
const ECHO_WINDOW: Duration = Duration::from_millis(300);
/// The capture thread looks at its control channel every 50 ms.
const GRAB_SETTLE: Duration = Duration::from_millis(300);
const UNGRAB_DEADLINE: Duration = Duration::from_secs(2);
const DEADMAN_MARGIN: Duration = Duration::from_millis(200);
const POLL_STEP: Duration = Duration::from_millis(10);

fn key_a(down: bool) -> InputEvent {
    InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage: KEY_A_USAGE,
            down,
            character: String::new(),
        })),
    }
}

/// A second client tries to take the keyboard; releases it again on success.
fn other_client_can_grab(conn: &RustConnection, root: Window) -> bool {
    let status = conn
        .grab_keyboard(false, root, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)
        .unwrap()
        .reply()
        .unwrap()
        .status;
    if status != GrabStatus::SUCCESS {
        return false;
    }
    conn.ungrab_keyboard(CURRENT_TIME).unwrap();
    conn.flush().unwrap();
    true
}

fn wait_until_other_can_grab(conn: &RustConnection, root: Window) -> bool {
    let deadline = Instant::now() + UNGRAB_DEADLINE;
    while Instant::now() < deadline {
        if other_client_can_grab(conn, root) {
            return true;
        }
        sleep(POLL_STEP);
    }
    false
}

#[test]
#[ignore = "needs an X server"]
fn start_under_xvfb_injection_moves_pointer_and_is_not_echoed_as_capture() {
    // Arrange
    let (mut capture, mut injector) = start().expect("backend starts under Xvfb");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    injector
        .commands
        .blocking_send(Inject::Warp { x: 100, y: 100 })
        .unwrap();
    let warped = injector.pointer.blocking_recv().unwrap();

    // Act
    injector
        .commands
        .blocking_send(Inject::Motion { dx: 5, dy: 3 })
        .unwrap();
    let moved = injector.pointer.blocking_recv().unwrap();
    for down in [true, false] {
        injector
            .commands
            .blocking_send(Inject::Event(key_a(down)))
            .unwrap();
    }
    let echoed =
        runtime.block_on(async { tokio::time::timeout(ECHO_WINDOW, capture.events.recv()).await });

    // Assert
    assert!(matches!(
        warped,
        Captured::Pointer {
            x: 100,
            y: 100,
            dx: 0,
            dy: 0
        }
    ));
    assert!(matches!(
        moved,
        Captured::Pointer {
            x: 105,
            y: 103,
            dx: 5,
            dy: 3
        }
    ));
    assert!(
        echoed.is_err(),
        "own injection came back as capture: {echoed:?}"
    );
}

#[test]
#[ignore = "needs an X server"]
fn start_under_xvfb_grab_holds_keyboard_until_released_or_deadman_expires() {
    // Arrange
    let (capture, _injector) = start().expect("backend starts under Xvfb");
    let (other, screen_num) = x11rb::connect(None).unwrap();
    let root = other.setup().roots[screen_num].root;

    // Act and assert: an explicit grab blocks other clients until Grab(false).
    capture
        .control
        .blocking_send(CaptureControl::Grab(true))
        .unwrap();
    capture
        .control
        .blocking_send(CaptureControl::Keepalive)
        .unwrap();
    sleep(GRAB_SETTLE);
    assert!(!other_client_can_grab(&other, root), "grab did not hold");
    capture
        .control
        .blocking_send(CaptureControl::Grab(false))
        .unwrap();
    assert!(
        wait_until_other_can_grab(&other, root),
        "Grab(false) leaked"
    );

    // Act and assert: without keepalives the dead-man switch releases the grab.
    capture
        .control
        .blocking_send(CaptureControl::Grab(true))
        .unwrap();
    sleep(GRAB_SETTLE);
    assert!(
        !other_client_can_grab(&other, root),
        "second grab did not hold"
    );
    sleep(DEADMAN_TIMEOUT + DEADMAN_MARGIN);
    assert!(
        other_client_can_grab(&other, root),
        "dead-man did not release"
    );
}
