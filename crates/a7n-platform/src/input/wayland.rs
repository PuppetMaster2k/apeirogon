// SPDX-License-Identifier: GPL-3.0-or-later

//! Wayland input backend: xdg-desktop-portal sessions and libei.
//!
//! Two portal sessions, because the compositor decides who may capture or
//! inject:
//!
//! - `InputCapture` captures local input once the pointer pushes through a
//!   barrier on the outer edge of the screens.
//! - `RemoteDesktop` injects input from peers through libei.
//!
//! Both are consented to once (restore tokens in the daemon state directory
//! make later starts silent where the portal supports it). One thread runs a
//! current-thread tokio runtime that owns both sessions and both libei
//! connections, and talks to the daemon through the channels in [`super`].
//!
//! Limits of the portal model:
//!
//! - Capture only starts at a barrier, never on demand. While a peer drives
//!   this machine, local input is therefore not blocked unless the pointer
//!   happens to push through a barrier, and local hotkeys are not seen.
//! - Zone changes re-arm the barriers, but the bounds told to the daemon at
//!   start stay as they were.
//! - The clipboard portal shares the `RemoteDesktop` session; see
//!   `crate::clipboard::wayland`.
//! - Not verified on a live GNOME or KDE session; see
//!   `docs/checklists/input-wayland.md`.

mod buttons;
mod capture;
mod inject;
mod release;
mod scroll;
mod session;
mod tokens;
mod translate;
mod zones;

use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use tokio::sync::mpsc;

use super::{
    Bounds, CAPTURE_CONTROL_CAPACITY, CAPTURE_EVENT_CAPACITY, Capture, CaptureControl, Captured,
    INJECT_COMMAND_CAPACITY, INJECT_POINTER_CAPACITY, Inject, Injector, InputError,
};
use crate::clipboard::wayland::portal::Service;
use capture::Capturer;
use inject::Injecting;
use tokens::TokenStore;

/// The portals may show a consent dialog per session; waiting for a person has
/// to end somewhere.
const START_TIMEOUT: Duration = Duration::from_secs(120);
/// Bound on handing capture back and closing the sessions when the backend ends.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);
const THREAD_NAME: &str = "a7n-wayland-input";

/// The daemon's ends of the channels, owned by the backend thread.
struct Channels {
    events: mpsc::Sender<Captured>,
    control: mpsc::Receiver<CaptureControl>,
    commands: mpsc::Receiver<Inject>,
    pointer: mpsc::Sender<Captured>,
}

type Ready = Result<(Bounds, Bounds), InputError>;

pub(super) fn start() -> Result<(Capture, Injector), InputError> {
    let (events_tx, events_rx) = mpsc::channel(CAPTURE_EVENT_CAPACITY);
    let (control_tx, control_rx) = mpsc::channel(CAPTURE_CONTROL_CAPACITY);
    let (commands_tx, commands_rx) = mpsc::channel(INJECT_COMMAND_CAPACITY);
    let (pointer_tx, pointer_rx) = mpsc::channel(INJECT_POINTER_CAPACITY);
    let channels = Channels {
        events: events_tx,
        control: control_rx,
        commands: commands_rx,
        pointer: pointer_tx,
    };
    let (ready_tx, ready_rx) = std_mpsc::channel();
    thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || run_thread(&ready_tx, channels))
        .map_err(InputError::Thread)?;

    // The thread always answers within `START_TIMEOUT`; a closed channel means
    // it died.
    let (capture_bounds, inject_bounds) = ready_rx
        .recv()
        .map_err(|_| InputError::Portal("the input thread stopped during startup".to_owned()))??;
    let capture = Capture {
        bounds: capture_bounds,
        events: events_rx,
        control: control_tx,
    };
    let injector = Injector {
        bounds: inject_bounds,
        commands: commands_tx,
        pointer: pointer_rx,
    };
    Ok((capture, injector))
}

fn run_thread(ready: &std_mpsc::Sender<Ready>, channels: Channels) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    match runtime {
        Ok(runtime) => runtime.block_on(run(ready, channels)),
        Err(error) => {
            let _ = ready.send(Err(InputError::Thread(error)));
        }
    }
}

async fn run(ready: &std_mpsc::Sender<Ready>, mut channels: Channels) {
    let mut tokens = TokenStore::open_default();
    session::register_app().await;
    let started = tokio::time::timeout(START_TIMEOUT, open(&mut tokens)).await;
    let (mut capturer, mut injecting) = match started {
        Ok(Ok(backend)) => backend,
        Ok(Err(error)) => {
            let _ = ready.send(Err(error));
            return;
        }
        Err(_) => {
            let _ = ready.send(Err(InputError::Portal(format!(
                "timed out after {}s waiting for the portal sessions to start",
                START_TIMEOUT.as_secs()
            ))));
            return;
        }
    };
    let mut clipboard = injecting.take_clipboard();
    if let Some(service) = &mut clipboard {
        // The slot is filled before the daemon learns that input is up.
        service.hand_over();
    }
    let bounds = (capturer.bounds(), capturer.bounds());
    if ready.send(Ok(bounds)).is_err() {
        // Nobody is waiting any more.
        close(&mut capturer, &mut injecting).await;
        return;
    }

    let outcome = tokio::select! {
        result = capturer.run(&channels.events, &mut channels.control) => result,
        result = injecting.run(&mut channels.commands, &channels.pointer) => result,
        () = run_clipboard(clipboard) => unreachable!("the clipboard service never ends the backend"),
    };
    if let Err(error) = outcome {
        eprintln!("kind=input_wayland_stopped reason={error}");
    }
    close(&mut capturer, &mut injecting).await;
}

/// Serves the clipboard portal next to input. When the service ends, or never
/// existed, input carries on alone.
async fn run_clipboard(service: Option<Service>) {
    if let Some(service) = service {
        service.run().await;
    }
    std::future::pending::<()>().await;
}

/// Starts both sessions, capture first. If the second fails the first is
/// closed again.
async fn open(tokens: &mut Option<TokenStore>) -> Result<(Capturer, Injecting), InputError> {
    let mut capturer = Capturer::new(session::open_capture(tokens).await?).await?;
    let injecting = async {
        let desktop = session::open_desktop(tokens).await?;
        Injecting::new(desktop, capturer.bounds()).await
    }
    .await;
    match injecting {
        Ok(injecting) => Ok((capturer, injecting)),
        Err(error) => {
            let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, capturer.shutdown()).await;
            Err(error)
        }
    }
}

async fn close(capturer: &mut Capturer, injecting: &mut Injecting) {
    // Capture first: it is the one that can lock the local user out.
    let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, capturer.shutdown()).await;
    let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, injecting.shutdown()).await;
}
