// SPDX-License-Identifier: GPL-3.0-or-later

//! Starts input once the user grants the macOS permissions it was missing.

use std::time::Duration;

use a7n_platform::permissions;
use tokio::sync::oneshot;

use crate::daemon::InputRetried;

/// Local daemon policy: how soon after a grant in System Settings input starts.
const INPUT_RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// Waits until no macOS permission is missing, then starts input and delivers
/// the handles, or the reason it still fails, to the daemon.
///
/// Only the permissions are polled: other platforms report none, and a declined
/// Wayland portal dialog must not be asked again every few seconds. Gives up on
/// any start failure that waiting cannot fix, and when the daemon stops listening.
pub(crate) fn spawn(handles: oneshot::Sender<InputRetried>) {
    tokio::spawn(retry(handles));
}

async fn retry(mut handles: oneshot::Sender<InputRetried>) {
    while !permissions::missing().is_empty() {
        tokio::select! {
            () = handles.closed() => return,
            () = tokio::time::sleep(INPUT_RETRY_INTERVAL) => {}
        }
    }
    let started = a7n_platform::input::start().map_err(|error| {
        crate::log::event!("kind=input_retry_stopped reason={error}");
        error.to_string()
    });
    let _ = handles.send(started);
}
