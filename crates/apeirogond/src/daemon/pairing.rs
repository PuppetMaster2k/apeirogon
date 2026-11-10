// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use a7n_core::pairing::{AttemptLimiter, HostPairing, Paired, PairingCode, RequesterPairing};
use a7n_proto::v1::PairDone;
use quinn::Connection;
use rand_core::OsRng;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

use super::{Daemon, DaemonError, Event, unix_seconds};
use crate::command::Reply;
use crate::pairing::{self, PairingError};
use crate::pin_store::Entry;
use crate::tls;

/// Bound the number of source-address rate-limit buckets.
const PAIRING_SOURCES: usize = 256;

pub(super) struct State {
    pub host: Option<HostPairing>,
    pub host_busy: bool,
    pub limiter: AttemptLimiter<IpAddr>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            host: None,
            host_busy: false,
            limiter: AttemptLimiter::new(PAIRING_SOURCES),
        }
    }
}

impl Daemon {
    pub(super) fn host_pairing(
        &mut self,
        connection: Connection,
        permit: OwnedSemaphorePermit,
    ) -> Result<(), DaemonError> {
        // Refused or rate-limited attempts must not close the window, or any
        // LAN host could cancel pairing for everyone.
        let open = self
            .trust
            .read()
            .map_err(|_| DaemonError::TrustPoisoned)?
            .pairing_open();
        let allowed = open
            && self.pairing.host.is_some()
            && !self.pairing.host_busy
            && self
                .pairing
                .limiter
                .allow(connection.remote_address().ip(), Instant::now());
        let Some(host) = allowed.then(|| self.pairing.host.take()).flatten() else {
            connection.close(crate::link::CLOSE_NORMAL, b"");
            return Ok(());
        };
        self.trust
            .write()
            .map_err(|_| DaemonError::TrustPoisoned)?
            .pairing_until = None;
        self.pairing.host_busy = true;
        let info = self.info.clone();
        let events = self.mailboxes.events.clone();
        self.tasks.spawn(async move {
            let admission_events = events.clone();
            let result = pairing::respond(
                connection,
                host,
                info,
                move |entry, paired| async move {
                    let (reply, receiver) = oneshot::channel();
                    admission_events
                        .send(Event::Confirmed {
                            entry,
                            paired,
                            reply,
                        })
                        .await
                        .map_err(|_| PairingError::Refused)?;
                    receiver.await.map_err(|_| PairingError::Refused)?
                },
                Instant::now,
                &mut OsRng,
            )
            .await;
            drop(permit);
            if result.is_err() {
                crate::log::event!("kind=pairing_failed");
            }
            let _ = events.send(Event::PairingEnded).await;
        });
        Ok(())
    }

    pub(super) fn admit(
        &mut self,
        entry: &Entry,
        paired: Paired,
    ) -> Result<PairDone, PairingError> {
        let result = unix_seconds().and_then(|now| {
            self.desk
                .admit(&self.identity, &mut self.pins, entry, now)?;
            Ok(())
        });
        let trust = self.sync_trust();
        self.drop_untrusted();
        self.broadcast(&a7n_proto::v1::Envelope {
            body: Some(a7n_proto::v1::envelope::Body::Desk(self.desk.to_desk())),
        });
        if result.is_err() || trust.is_err() {
            crate::log::event!("kind=pairing_store_failed");
            return Err(PairingError::Refused);
        }
        Ok(paired.done())
    }

    pub(super) fn join(&mut self, addr: SocketAddr, code: PairingCode, reply: Reply<Entry>) {
        let config = match tls::pair_client_config(&self.identity) {
            Ok(config) => config,
            Err(error) => {
                let _ = reply.send(Err(error.into()));
                return;
            }
        };
        let mut endpoint = self.endpoint.clone();
        endpoint.set_default_client_config(config);
        let requester =
            RequesterPairing::new(code, None, self.identity.pin, self.info.clone(), &mut OsRng);
        let events = self.mailboxes.events.clone();
        self.tasks.spawn(async move {
            let result = async {
                let connection = pairing::dial(&endpoint, addr).await?;
                pairing::initiate(connection, requester).await
            }
            .await;
            let _ = events
                .send(Event::Joined {
                    result,
                    addr,
                    reply,
                })
                .await;
        });
    }

    pub(super) fn joined(
        &mut self,
        result: Result<Entry, PairingError>,
        addr: SocketAddr,
        reply: Reply<Entry>,
    ) {
        let result = result.map_err(DaemonError::from).and_then(|entry| {
            self.store_peer(entry.clone())?;
            Ok(entry)
        });
        match result {
            Ok(entry) => {
                let (connected, receiver) = oneshot::channel();
                self.connect(addr, connected);
                self.tasks.spawn(async move {
                    if !matches!(receiver.await, Ok(Ok(()))) {
                        crate::log::event!("kind=post_pair_connect_failed");
                    }
                    let _ = reply.send(Ok(entry));
                });
            }
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        }
    }

    pub(super) fn store_peer(&mut self, entry: Entry) -> Result<(), DaemonError> {
        if entry.machine_id == self.identity.machine_id {
            return Err(PairingError::Refused.into());
        }
        let mut staged = self.pins.clone();
        staged.add(entry);
        staged.save()?;
        self.pins = staged;
        self.sync_trust()
    }
}
