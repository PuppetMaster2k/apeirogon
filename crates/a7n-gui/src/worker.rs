// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::mpsc;
use std::time::Duration;

use a7n_ipc::config::Config;
use a7n_ipc::{IpcClient, IpcError, MachineId, Request, Response};
use eframe::egui;
use rust_i18n::t;
use tokio::sync::mpsc as async_mpsc;

const MAILBOX_CAPACITY: usize = 32;
const STATUS_INTERVAL: Duration = Duration::from_secs(1);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
// The daemon records for up to 10 seconds; leave time for its reply.
const RECORD_HOTKEY_TIMEOUT: Duration = Duration::from_secs(15);
const LONG_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) enum Command {
    Request(Request),
    Config,
    SaveConfig(Vec<(String, String)>),
    Start,
    CheckUpdates,
}

#[allow(
    clippy::large_enum_variant,
    reason = "one update per second reaches the UI thread; boxing would only add noise"
)]
pub(super) enum Update {
    Response(Result<Response, String>),
    Config(Result<Box<Config>, String>),
    Notice(String),
    Recorded(Result<String, String>),
    Release(Result<Option<crate::update_check::Release>, String>),
}

pub(super) struct Worker {
    pub(super) commands: async_mpsc::Sender<Command>,
    pub(super) updates: mpsc::Receiver<Update>,
}

impl Worker {
    pub(super) fn new(ctx: egui::Context) -> std::io::Result<Self> {
        let (commands, mut receiver) = async_mpsc::channel(MAILBOX_CAPACITY);
        let (sender, updates) = mpsc::sync_channel(MAILBOX_CAPACITY);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        std::thread::Builder::new().name("gui-ipc".into()).spawn(move || {
            let publish = |update| {
                let sent = sender.send(update).is_ok();
                ctx.request_repaint();
                sent
            };
            runtime.block_on(async {
                let mut client = None;
                let mut poll = tokio::time::interval(STATUS_INTERVAL);
                poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    let command = tokio::select! {
                        command = receiver.recv() => match command { Some(command) => command, None => break },
                        _ = poll.tick() => Command::Request(Request::Status),
                    };
                    let update = match command {
                        Command::CheckUpdates => {
                            let sender = sender.clone();
                            let ctx = ctx.clone();
                            tokio::task::spawn_blocking(move || {
                                let _ = sender.send(Update::Release(crate::update_check::check()));
                                ctx.request_repaint();
                            });
                            continue;
                        }
                        Command::Request(Request::RecordHotkey { kind }) => {
                            Update::Recorded(match request_daemon(&mut client, &Request::RecordHotkey { kind }).await {
                                Ok(Response::Recorded { value }) => Ok(value),
                                Ok(Response::Error { message, .. }) | Err(message) => Err(message),
                                Ok(_) => Err(t!("Unexpected daemon response").into()),
                            })
                        }
                        Command::Request(Request::Unpair { machine }) => {
                            let Some(update) = unpair(&mut client, machine, &publish).await else {
                                break;
                            };
                            update
                        }
                        Command::Request(request) => Update::Response(request_daemon(&mut client, &request).await),
                        Command::Config => Update::Config(load_config().map(Box::new)),
                        Command::SaveConfig(values) => {
                            match save_config(&values) {
                                Ok(config) => {
                                    if !publish(Update::Config(Ok(Box::new(config)))) { break; }
                                    match request_daemon(&mut client, &Request::ReloadConfig).await {
                                        Ok(response) => Update::Response(Ok(response)),
                                        Err(error) => Update::Notice(t!("Saved; reload failed: %{error}. Applies at next start.", error = error).into_owned()),
                                    }
                                }
                                Err(error) => Update::Notice(t!("Could not save config: %{error}", error = error).into_owned()),
                            }
                        }
                        Command::Start => match a7n_ipc::daemon::start().await {
                            Ok(_) => Update::Response(request_daemon(&mut client, &Request::Status).await),
                            Err(error) => Update::Notice(t!("Could not start daemon: %{error}", error = error).into_owned()),
                        },
                    };
                    if !publish(update) { break; }
                }
            });
        })?;
        Ok(Self { commands, updates })
    }
}

async fn unpair(
    client: &mut Option<IpcClient>,
    machine: MachineId,
    publish: &impl Fn(Update) -> bool,
) -> Option<Update> {
    let response = request_daemon(client, &Request::Unpair { machine }).await;
    if !matches!(response, Ok(Response::Done)) {
        return Some(Update::Response(response));
    }
    if !publish(Update::Notice(t!("Machine unpaired").into())) {
        return None;
    }
    Some(Update::Response(
        request_daemon(client, &Request::Status).await,
    ))
}

async fn request_daemon(
    client: &mut Option<IpcClient>,
    request: &Request,
) -> Result<Response, String> {
    if client.is_none() {
        *client = Some(IpcClient::connect().await.map_err(|e| e.to_string())?);
    }
    let timeout = match request {
        Request::RecordHotkey { .. } => RECORD_HOTKEY_TIMEOUT,
        Request::PairHost | Request::PairJoin { .. } | Request::SendFiles { .. } => {
            LONG_REQUEST_TIMEOUT
        }
        _ => REQUEST_TIMEOUT,
    };
    let result = tokio::time::timeout(
        timeout,
        client
            .as_mut()
            .expect("client connected above")
            .request(request),
    )
    .await
    .unwrap_or(Err(IpcError::Timeout));
    if result.is_err() {
        *client = None;
    }
    result.map_err(|e| e.to_string())
}

fn load_config() -> Result<Config, String> {
    Config::path()
        .and_then(|path| Config::load(&path))
        .map_err(|e| t!("Could not load config: %{error}", error = e).into_owned())
}

fn save_config(values: &[(String, String)]) -> Result<Config, String> {
    let path = Config::path().map_err(|e| e.to_string())?;
    let mut config = Config::load(&path).map_err(|e| e.to_string())?;
    config
        .set_many(
            values
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .map_err(|e| e.to_string())?;
    config.save(&path).map_err(|e| e.to_string())?;
    Ok(config)
}
