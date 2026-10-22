// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::Ipv4Addr;
use std::time::Duration;

use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::*;

pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) struct Peer {
    pub(super) sender: Option<mpsc::Sender<Command>>,
    pub(super) discoveries: mpsc::Sender<crate::discovery::Event>,
    task: JoinHandle<Result<(), DaemonError>>,
    pub(crate) machine: MachineId,
    pub(crate) addr: SocketAddr,
    // Keep persistence alive until the daemon task is stopped.
    pub(crate) dir: TempDir,
}

impl Peer {
    pub(crate) fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        Self::run(daemon, dir)
    }

    pub(crate) fn run(daemon: Daemon, dir: TempDir) -> Self {
        let machine = daemon.identity.machine_id;
        let addr = daemon.endpoint.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel(MAILBOX_CAPACITY);
        let (discoveries, discovery_rx) = mpsc::channel(MAILBOX_CAPACITY);
        let task = tokio::spawn(daemon.run(receiver, discovery_rx));
        Self {
            sender: Some(sender),
            discoveries,
            task,
            machine,
            addr,
            dir,
        }
    }

    pub(crate) async fn command<T>(
        &self,
        command: impl FnOnce(Reply<T>) -> Command,
    ) -> Result<T, DaemonError> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .as_ref()
            .unwrap()
            .send(command(reply))
            .await
            .unwrap();
        receiver.await.unwrap()
    }

    pub(crate) async fn status(&self) -> Status {
        self.command(Command::Status).await.unwrap()
    }

    pub(crate) async fn wait(&self, predicate: impl Fn(&Status) -> bool) -> Status {
        timeout(TEST_TIMEOUT, async {
            loop {
                let status = self.status().await;
                if predicate(&status) {
                    return status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("daemon did not reach expected state")
    }

    pub(crate) async fn pair(&self, requester: &Self) -> Entry {
        let code = self.command(Command::PairHost).await.unwrap();
        let entry = timeout(
            TEST_TIMEOUT,
            requester.command(|reply| Command::PairJoin {
                addr: self.addr,
                code,
                reply,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        self.wait(|status| linked(status, requester.machine)).await;
        requester.wait(|status| linked(status, self.machine)).await;
        entry
    }

    pub(crate) async fn stop(mut self) {
        self.sender.take();
        timeout(TEST_TIMEOUT, &mut self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) fn linked(status: &Status, machine: MachineId) -> bool {
    status
        .peers
        .iter()
        .any(|peer| peer.id == machine && peer.connected)
}

pub(super) fn paired(status: &Status, machine: MachineId) -> bool {
    status.peers.iter().any(|peer| peer.id == machine)
}
