// SPDX-License-Identifier: GPL-3.0-or-later

//! Pasteboard ownership and provider callbacks stay on a dedicated `CFRunLoop`.

mod provider;

use std::ffi::c_void;
use std::io;
use std::sync::mpsc as sync;
use std::thread;
use std::time::Duration;

use a7n_core::clipboard::{Item, LocalChange, TEXT_HTML, TEXT_PLAIN};
use a7n_proto::limits::CLIPBOARD_TEXT_MAX;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSWorkspace};
use objc2_core_foundation::{
    CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext, kCFRunLoopDefaultMode,
};
use objc2_foundation::{NSArray, NSData, NSString, ns_string};
use tokio::sync::mpsc;

use super::{Clipboard, ClipboardCommand, PasteRequest};

const CHANNEL_CAPACITY: usize = 16;
const POLL_INTERVAL: Duration = Duration::from_millis(250);

struct State {
    board: Retained<NSPasteboard>,
    commands: mpsc::Receiver<ClipboardCommand>,
    changes: mpsc::Sender<LocalChange>,
    pastes: mpsc::Sender<PasteRequest>,
    sequence: Option<isize>,
    provider: Option<Retained<provider::Provider>>,
}

pub(super) fn start() -> io::Result<Clipboard> {
    let (changes_tx, changes) = mpsc::channel(CHANNEL_CAPACITY);
    let (commands, commands_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (pastes_tx, pastes) = mpsc::channel(CHANNEL_CAPACITY);
    let (ready_tx, ready) = sync::channel();
    thread::Builder::new()
        .name("a7n-macos-clipboard".into())
        .spawn(move || {
            autoreleasepool(|_| {
                let mut state = State {
                    board: NSPasteboard::generalPasteboard(),
                    commands: commands_rx,
                    changes: changes_tx,
                    pastes: pastes_tx,
                    sequence: None,
                    provider: None,
                };
                let result = run(&mut state, &ready_tx);
                if let Err(error) = result {
                    let _ = ready_tx.send(Err(error));
                }
            });
        })?;
    ready.recv().map_err(io::Error::other)??;
    Ok(Clipboard {
        changes,
        commands,
        pastes,
    })
}

fn run(state: &mut State, ready: &sync::Sender<io::Result<()>>) -> io::Result<()> {
    let run_loop = CFRunLoop::current().ok_or_else(|| io::Error::other("missing run loop"))?;
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: std::ptr::from_mut(state).cast(),
        retain: None,
        release: None,
        copyDescription: None,
    };
    // SAFETY: The default allocator accepts None. The context points to state,
    // which stays alive on this thread until the timer is invalidated below.
    let timer = unsafe {
        CFRunLoopTimer::new(
            None,
            0.0,
            POLL_INTERVAL.as_secs_f64(),
            0,
            0,
            Some(tick),
            &raw mut context,
        )
    }
    .ok_or_else(|| io::Error::other("cannot create clipboard timer"))?;
    // SAFETY: Core Foundation's default run-loop mode is an immutable global.
    run_loop.add_timer(Some(&timer), unsafe { kCFRunLoopDefaultMode });
    if ready.send(Ok(())).is_ok() {
        // Providers need the declaring thread's run loop to service callbacks.
        // AppKit's background-thread delivery must also be verified on macOS;
        // do not replace delayed rendering with eager fetching if it requires main.
        CFRunLoop::run();
    }
    timer.invalidate();
    if let Some(provider) = state.provider.take() {
        provider.finish();
    }
    Ok(())
}

unsafe extern "C-unwind" fn tick(_: *mut CFRunLoopTimer, info: *mut c_void) {
    autoreleasepool(|_| {
        // SAFETY: run installs this pointer to live, exclusively owned State;
        // this timer only runs on that thread and is invalidated before return.
        let state = unsafe { &mut *info.cast::<State>() };
        for _ in 0..CHANNEL_CAPACITY {
            match state.commands.try_recv() {
                Ok(command) => state.set(command),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if let Some(run_loop) = CFRunLoop::current() {
                        run_loop.stop();
                    }
                    return;
                }
            }
        }
        state.changed();
    });
}

impl State {
    fn changed(&mut self) {
        let sequence = self.board.changeCount();
        if self.sequence == Some(sequence) {
            return;
        }
        if let Some(provider) = self.provider.take() {
            provider.finish();
        }
        let Some(types) = self.board.types() else {
            self.sequence = Some(sequence);
            return;
        };
        let sensitive = types.containsObject(ns_string!("org.nspasteboard.ConcealedType"))
            || types.containsObject(ns_string!("org.nspasteboard.TransientType"));
        let mut change = LocalChange {
            items: Vec::new(),
            sensitive,
            // shortcut: frontmost app is best effort, use owner attribution if AppKit gains it.
            source_app: NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .and_then(|app| app.bundleIdentifier())
                .map(|id| id.to_string()),
        };
        if !sensitive {
            for (mime, uti) in formats() {
                if let Some(data) = self.board.dataForType(uti) {
                    if data.length() <= CLIPBOARD_TEXT_MAX {
                        change.items.push(Item {
                            mime: mime.into(),
                            data: data.to_vec(),
                        });
                    }
                }
            }
        }
        // Another app may replace the pasteboard while a data provider runs.
        if self.board.changeCount() == sequence && self.changes.try_send(change).is_ok() {
            self.sequence = Some(sequence);
        }
    }

    fn set(&mut self, command: ClipboardCommand) {
        let ClipboardCommand::Set {
            generation,
            items,
            lazy,
        } = command;
        let item = NSPasteboardItem::new();
        for entry in items {
            if entry.data.len() <= CLIPBOARD_TEXT_MAX {
                if let Some(uti) = uti(&entry.mime) {
                    item.setData_forType(&NSData::with_bytes(&entry.data), uti);
                }
            }
        }
        let eager = item.types();
        let mut offered = Vec::new();
        let lazy: Vec<_> = lazy
            .into_iter()
            .filter(|entry| {
                let Some(uti) = uti(&entry.mime) else {
                    return false;
                };
                if entry.size > CLIPBOARD_TEXT_MAX as u64
                    || eager.containsObject(uti)
                    || offered.contains(&uti)
                {
                    return false;
                }
                offered.push(uti);
                true
            })
            .collect();
        let provider = provider::Provider::new(generation, lazy, self.pastes.clone());
        if !offered.is_empty() {
            item.setDataProvider_forTypes(
                ProtocolObject::from_ref(&*provider),
                &NSArray::from_slice(&offered),
            );
        }
        if let Some(old) = self.provider.take() {
            old.finish();
        }
        // Keep our returned ownership count, never a later owner's count.
        self.sequence = Some(self.board.clearContents());
        if self
            .board
            .writeObjects(&NSArray::from_slice(&[ProtocolObject::from_ref(&*item)]))
        {
            self.provider = Some(provider);
        }
    }
}

fn formats() -> [(&'static str, &'static NSString); 2] {
    [
        (TEXT_PLAIN, ns_string!("public.utf8-plain-text")),
        (TEXT_HTML, ns_string!("public.html")),
    ]
}

fn uti(mime: &str) -> Option<&'static NSString> {
    formats()
        .into_iter()
        .find_map(|(supported, uti)| (mime == supported).then_some(uti))
}
