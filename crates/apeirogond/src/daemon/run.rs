// SPDX-License-Identifier: GPL-3.0-or-later

use std::ops::ControlFlow;
use std::time::{Duration, Instant, SystemTime};

use a7n_proto::{MachineId, v1::Envelope};
use tokio::sync::{OwnedSemaphorePermit, mpsc};

use super::{Daemon, DaemonError, MAILBOX_CAPACITY, clipboard, files, input, nearby, sync};
use crate::{command::Command, endpoint};

type Accepted = (
    quinn::Connection,
    OwnedSemaphorePermit,
    tokio::time::Instant,
);

impl Daemon {
    pub(crate) async fn run(
        mut self,
        commands: mpsc::Receiver<Command>,
        discoveries: mpsc::Receiver<crate::discovery::Event>,
    ) -> Result<(), DaemonError> {
        let result = self.run_loop(commands, discoveries).await;
        self.shutdown_input().await;
        if let Some(reply) = self.shutdown_reply.take() {
            let _ = reply.send(Ok(()));
        }
        result
    }

    async fn run_loop(
        &mut self,
        mut commands: mpsc::Receiver<Command>,
        mut discoveries: mpsc::Receiver<crate::discovery::Event>,
    ) -> Result<(), DaemonError> {
        let mut accepted = self.start_run()?;
        let mut retry = interval(nearby::RETRY_INTERVAL);
        let mut sync = interval(sync::SYNC_MIN_INTERVAL);
        let mut keepalive = interval(a7n_platform::input::KEEPALIVE_INTERVAL);
        loop {
            if self
                .run_next(
                    &mut commands,
                    &mut discoveries,
                    &mut accepted,
                    &mut retry,
                    &mut sync,
                    &mut keepalive,
                )
                .await?
                .is_break()
            {
                return Ok(());
            }
            self.finish_iteration().await?;
        }
    }

    fn start_run(&mut self) -> Result<mpsc::Receiver<Accepted>, DaemonError> {
        self.detect_settings_edit()?;
        if let Ok(receive_dir) = self.config.files.receive_dir() {
            self.tasks
                .spawn_blocking(move || files::cleanup(&receive_dir, SystemTime::now()));
        }
        if self.clipboard.handle.is_none() {
            crate::log::event!("kind=clipboard_unavailable");
        }
        let (accepted, accepted_rx) = mpsc::channel(MAILBOX_CAPACITY);
        self.tasks
            .spawn(endpoint::accept_loop(self.endpoint.clone(), accepted));
        Ok(accepted_rx)
    }

    async fn run_next(
        &mut self,
        commands: &mut mpsc::Receiver<Command>,
        discoveries: &mut mpsc::Receiver<crate::discovery::Event>,
        accepted_rx: &mut mpsc::Receiver<Accepted>,
        retry: &mut tokio::time::Interval,
        sync: &mut tokio::time::Interval,
        keepalive: &mut tokio::time::Interval,
    ) -> Result<ControlFlow<()>, DaemonError> {
        let deadline = self.input.focus.next_deadline();
        let files_deadline = self.files_deadline();
        let clipboard_deadline = self.clipboard.held.as_ref().map(|held| held.deadline);
        let wake_deadline = self.wol.pending.as_ref().map(|pending| pending.deadline);
        tokio::select! {
            command = commands.recv() => {
                return self.run_command(command).await;
            }
            Some((connection, permit, deadline)) = accepted_rx.recv() => {
                self.accept(connection, permit, deadline)?;
            }
            Some(event) = self.mailboxes.event_rx.recv() => self.event(event),
            Some(event) = discoveries.recv() => self.discovery(event),
            _ = retry.tick() => self.retry_nearby(),
            _ = sync.tick() => self.flush_sync(Instant::now()).await,
            _ = keepalive.tick() => self.input.keepalive(),
            () = input::deadline(wake_deadline) => {},
            () = input::deadline(deadline) => self.poll_focus().await,
            () = input::deadline(clipboard_deadline) => self.clipboard.expire_held_clipboard(Instant::now()),
            () = input::deadline(files_deadline) => self.expire_files(Instant::now()),
            Some(stream) = self.mailboxes.transfer_rx.recv() => self.clipboard_stream(stream),
            event = clipboard::next(&mut self.clipboard.handle) => match event {
                clipboard::Event::Changed(change) => self.clipboard_changed(change),
                clipboard::Event::Paste(request) => self.clipboard_paste(request),
            },
            retried = input::retried(&mut self.input.retried) => self.finish_input_retry(retried),
            event = input::captured(&mut self.input.capture) => self.captured(event).await,
            event = input::feedback(&mut self.input.injector) => self.pointer_feedback(event).await,
            Some((peer, id, envelope)) = self.mailboxes.inbound_rx.recv() => {
                self.receive_inbound(peer, id, envelope).await;
            }
            Some(_) = self.tasks.join_next(), if !self.tasks.is_empty() => {
                self.start_waiting_transfers();
            }
        }
        Ok(ControlFlow::Continue(()))
    }

    async fn run_command(
        &mut self,
        command: Option<Command>,
    ) -> Result<ControlFlow<()>, DaemonError> {
        let Some(command) = command else {
            return Ok(ControlFlow::Break(()));
        };
        if let Command::Shutdown(reply) = command {
            self.shutdown_reply = Some(reply);
            return Ok(ControlFlow::Break(()));
        }
        self.command(command).await?;
        Ok(ControlFlow::Continue(()))
    }

    async fn receive_inbound(&mut self, peer: MachineId, id: usize, envelope: Envelope) {
        if self.is_current(peer, id) && self.receive_from(peer, envelope).await.is_err() {
            self.peer_message_failed(peer);
        }
    }

    fn start_waiting_transfers(&mut self) {
        // Aborted clipboard tasks release slots without a completion event.
        let peers: Vec<_> = self.links.by_peer.keys().copied().collect();
        for peer in peers {
            self.start_waiting_files(peer);
        }
    }

    async fn finish_iteration(&mut self) -> Result<(), DaemonError> {
        self.finish_wake(Instant::now()).await;
        self.flush_link_losses().await;
        if self.input.failed && !a7n_platform::permissions::missing().is_empty() {
            self.retry_input_after_permission_loss().await;
        }
        if self.input.failed {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "input backend closed",
            )
            .into());
        }
        Ok(())
    }
}

fn interval(period: Duration) -> tokio::time::Interval {
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}
