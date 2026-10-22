// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{self, IsTerminal};

use a7n_ipc::{Request, Response};
use anyhow::{Result, bail};
use crossterm::event::{self, Event};

mod draw;
mod editor;

use editor::{Action, Editor};

pub(super) async fn run() -> Result<()> {
    if !io::stdout().is_terminal() {
        bail!("a7n layout requires an interactive terminal on stdout");
    }
    let mut client = super::connect_client().await?;
    let mut editor = match client.request(&Request::GetLayout).await? {
        Response::Layout(info) => Editor::new(info)?,
        Response::Error { message, .. } => bail!("{message}"),
        _ => bail!("expected layout from daemon"),
    };
    drop(client);
    let Some(placed) = edit(&mut editor)? else {
        return Ok(());
    };
    // Editing can outlast the IPC idle timeout.
    let mut client = super::connect_client().await?;
    match client.request(&Request::SetLayout { placed }).await? {
        Response::Done => Ok(()),
        Response::Error { message, .. } => bail!("{message}"),
        _ => bail!("expected layout save acknowledgement from daemon"),
    }
}

fn edit(editor: &mut Editor) -> Result<Option<Vec<a7n_ipc::LayoutPlacement>>> {
    let _restore = RestoreTerminal;
    let mut terminal = ratatui::try_init()?;
    loop {
        terminal.draw(|frame| draw::draw(frame, editor))?;
        if let Event::Key(key) = event::read()? {
            match editor.handle(key) {
                Action::Continue => {}
                Action::Quit => return Ok(None),
                Action::Save(placed) => return Ok(Some(placed)),
            }
        }
    }
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
