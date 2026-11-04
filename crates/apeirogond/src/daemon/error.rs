// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::layout::LayoutError;
use a7n_ipc::config::ConfigError;

use crate::desk_store::DeskStoreError;
use crate::identity::IdentityError;
use crate::link::LinkError;
use crate::pairing::PairingError;
use crate::pin_store::PinStoreError;
use crate::tls::TlsError;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DaemonError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Pins(#[from] PinStoreError),
    #[error(transparent)]
    Desk(#[from] DeskStoreError),
    #[error(transparent)]
    Layout(#[from] LayoutError),
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error(transparent)]
    Link(#[from] LinkError),
    #[error(transparent)]
    Pairing(#[from] PairingError),
    #[error(transparent)]
    Permission(#[from] a7n_platform::permissions::PermissionError),
    #[error("system time precedes the Unix epoch")]
    Clock(#[from] std::time::SystemTimeError),
    #[error("pairing exchange already active")]
    PairingBusy,
    #[error("input sharing is paused")]
    Paused,
    #[error("machine is not paired")]
    NotPaired,
    #[error("{0}")]
    InvalidRequest(&'static str),
    #[error("machine is not connected")]
    NotConnected,
    #[error("not enough free space in the receive directory")]
    NoSpace,
    #[error("trust snapshot lock poisoned")]
    TrustPoisoned,
}
