// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;
use std::time::Instant;

use a7n_core::pairing::{Paired, PairingCode, RequesterPairing};
use a7n_proto::v1::PairDone;
use quinn::Connection;
use rand_core::OsRng;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

use super::{Daemon, DaemonError, Event, unix_seconds};
use crate::command::Reply;
use crate::pairing::{self, PairingError};
use crate::pin_store::Entry;
use crate::tls;

impl Daemon {
    pub(super) fn host_pairing(
        &mut self,
        connection: Connection,
        permit: OwnedSemaphorePermit,
    ) -> Result<(), DaemonError> {
        let open = {
            let mut snapshot = self.trust.write().map_err(|_| DaemonError::TrustPoisoned)?;
            let open = snapshot.pairing_open();
            snapshot.pairing_until = None;
            open
        };
        let host = self.host.take();
        let allowed = open
            && !self.host_busy
            && self
                .limiter
                .allow(connection.remote_address().ip(), Instant::now());
        let Some(host) = host.filter(|_| allowed) else {
            connection.close(0u32.into(), b"");
            return Ok(());
        };
        self.host_busy = true;
        let info = self.info.clone();
        let events = self.events.clone();
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
                eprintln!("kind=pairing_failed");
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
            eprintln!("kind=pairing_store_failed");
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
        let events = self.events.clone();
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

    pub(super) fn store_peer(&mut self, entry: Entry) -> Result<(), DaemonError> {
        let mut staged = self.pins.clone();
        staged.add(entry);
        staged.save()?;
        self.pins = staged;
        self.sync_trust()
    }
}
