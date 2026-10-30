// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::mpsc;
use std::time::Duration;

use a7n_ipc::config::Config;
use a7n_ipc::{IpcClient, Request, Response};
use eframe::egui;
use tokio::sync::mpsc as async_mpsc;

const MAILBOX_CAPACITY: usize = 32;
const STATUS_INTERVAL: Duration = Duration::from_secs(1);

pub(super) enum Command {
    Request(Request),
    Config,
    SaveConfig(Vec<(String, String)>),
    Start,
}

#[allow(
    clippy::large_enum_variant,
    reason = "one update per second reaches the UI thread; boxing would only add noise"
)]
pub(super) enum Update {
    Response(Result<Response, String>),
    Config(Result<Box<Config>, String>),
    Notice(String),
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
                        Command::Request(request) => Update::Response(request_daemon(&mut client, &request).await),
                        Command::Config => Update::Config(load_config().map(Box::new)),
                        Command::SaveConfig(values) => {
                            match save_config(&values) {
                                Ok(config) => {
                                    if !publish(Update::Config(Ok(Box::new(config)))) { break; }
                                    match request_daemon(&mut client, &Request::ReloadConfig).await {
                                        Ok(response) => Update::Response(Ok(response)),
                                        Err(error) => Update::Notice(format!("Saved; reload failed: {error}. Applies at next start.")),
                                    }
                                }
                                Err(error) => Update::Notice(error),
                            }
                        }
                        Command::Start => match a7n_ipc::daemon::start().await {
                            Ok(_) => Update::Response(request_daemon(&mut client, &Request::Status).await),
                            Err(error) => Update::Notice(error.to_string()),
                        },
                    };
                    if !publish(update) { break; }
                }
            });
        })?;
        Ok(Self { commands, updates })
    }
}

async fn request_daemon(
    client: &mut Option<IpcClient>,
    request: &Request,
) -> Result<Response, String> {
    if client.is_none() {
        *client = Some(IpcClient::connect().await.map_err(|e| e.to_string())?);
    }
    let result = client
        .as_mut()
        .expect("client connected above")
        .request(request)
        .await;
    if result.is_err() {
        *client = None;
    }
    result.map_err(|e| e.to_string())
}

fn load_config() -> Result<Config, String> {
    Config::path()
        .and_then(|path| Config::load(&path))
        .map_err(|e| e.to_string())
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
