// SPDX-License-Identifier: GPL-3.0-or-later

//! Pasteboard ownership and provider callbacks stay on a dedicated `CFRunLoop`.

mod provider;

use std::ffi::c_void;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc as sync;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use a7n_core::clipboard::{
    FILES, Generation, IMAGE_PNG, Item, LocalChange, TEXT_HTML, TEXT_PLAIN, limit_for, validate_png,
};
use a7n_proto::limits;
use objc2::ClassType;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardItem,
    NSPasteboardURLReadingFileURLsOnlyKey, NSWorkspace,
};
use objc2_core_foundation::{
    CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext, kCFRunLoopDefaultMode,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSNumber, NSString, NSURL, ns_string};
use tokio::sync::mpsc;

use super::{Clipboard, ClipboardCommand, LazyItem, PasteRequest};

const CHANNEL_CAPACITY: usize = 16;
const POLL_INTERVAL: Duration = Duration::from_millis(250);

struct State {
    board: Retained<NSPasteboard>,
    commands: mpsc::Receiver<ClipboardCommand>,
    changes: mpsc::Sender<LocalChange>,
    pastes: mpsc::Sender<PasteRequest>,
    sequence: Option<isize>,
    providers: Vec<Retained<provider::Provider>>,
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
                    providers: Vec::new(),
                };
                let result = run(&mut state, &ready_tx);
                if let Err(error) = result {
                    let _ = ready_tx.send(Err(error));
                }
            });
        })?;
    ready.recv().map_err(io::Error::other)??;
    Ok(Clipboard {
        images: true,
        files: true,
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
    finish_providers(&mut state.providers);
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
        finish_providers(&mut self.providers);
        let Some(types) = self.board.types() else {
            self.sequence = Some(sequence);
            return;
        };
        let sensitive = types.containsObject(ns_string!("org.nspasteboard.ConcealedType"))
            || types.containsObject(ns_string!("org.nspasteboard.TransientType"));
        let mut change = LocalChange {
            files: Vec::new(),
            items: Vec::new(),
            sensitive,
            // shortcut: frontmost app is best effort, use owner attribution if AppKit gains it.
            source_app: NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .and_then(|app| app.bundleIdentifier())
                .map(|id| id.to_string()),
        };
        if !sensitive {
            change.files = self.file_paths();
            for (mime, uti) in formats() {
                if let Some(data) = self.board.dataForType(uti) {
                    if data.length() <= limit_for(mime) {
                        change.items.push(Item {
                            mime: mime.into(),
                            data: data.to_vec(),
                        });
                    }
                }
            }
            if !change.items.iter().any(|item| item.mime == IMAGE_PNG) {
                if let Some(data) = self.board.dataForType(ns_string!("public.tiff")) {
                    if let Some(png) = convert_image(&data, NSBitmapImageFileType::PNG) {
                        if png.length() <= limits::IMAGE_CLIPBOARD_MAX {
                            change.items.push(Item {
                                mime: IMAGE_PNG.into(),
                                data: png.to_vec(),
                            });
                        }
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
            if entry.data.len() <= limit_for(&entry.mime) {
                if let Some(uti) = uti(&entry.mime) {
                    let data = NSData::with_bytes(&entry.data);
                    if entry.mime == IMAGE_PNG {
                        if validate_png(&entry.data).is_err() {
                            continue;
                        }
                        if let Some(tiff) = convert_image(&data, NSBitmapImageFileType::TIFF) {
                            item.setData_forType(&tiff, ns_string!("public.tiff"));
                        }
                    }
                    item.setData_forType(&data, uti);
                }
            }
        }
        let eager = item.types();
        let (files, lazy): (Vec<_>, Vec<_>) =
            lazy.into_iter().partition(|entry| entry.mime == FILES);
        let mut offered = Vec::new();
        let lazy: Vec<_> = lazy
            .into_iter()
            .filter(|entry| {
                let Some(uti) = uti(&entry.mime) else {
                    return false;
                };
                if !entry.within_limit() || eager.containsObject(uti) || offered.contains(&uti) {
                    return false;
                }
                offered.push(uti);
                if entry.mime == IMAGE_PNG {
                    offered.push(ns_string!("public.tiff"));
                }
                true
            })
            .collect();
        let has_content = eager.count() > 0 || !offered.is_empty();
        let mut providers = Vec::new();
        if !offered.is_empty() {
            let provider = provider::Provider::new(generation, lazy, self.pastes.clone());
            item.setDataProvider_forTypes(
                ProtocolObject::from_ref(&*provider),
                &NSArray::from_slice(&offered),
            );
            providers.push(provider);
        }
        let mut board_items = Vec::new();
        if has_content {
            board_items.push(item);
        }
        if let Some(files) = files.into_iter().next() {
            let (file_items, file_providers) = self.file_items(generation, &files);
            board_items.extend(file_items);
            providers.extend(file_providers);
        }
        if board_items.is_empty() {
            return;
        }
        finish_providers(&mut self.providers);
        // Keep our returned ownership count, never a later owner's count.
        self.sequence = Some(self.board.clearContents());
        let writing: Vec<_> = board_items
            .iter()
            .map(|item| ProtocolObject::from_ref(&**item))
            .collect();
        if self.board.writeObjects(&NSArray::from_slice(&writing)) {
            self.providers = providers;
        }
    }

    /// One pasteboard item per top-level entry, each answering with its file
    /// URL once something pastes it. A true `NSFilePromiseProvider` is for
    /// drags; a file URL provided on demand is what a paste accepts.
    fn file_items(
        &self,
        generation: Generation,
        files: &LazyItem,
    ) -> (
        Vec<Retained<NSPasteboardItem>>,
        Vec<Retained<provider::Provider>>,
    ) {
        let published = Arc::new(Mutex::new(None));
        let mut items = Vec::new();
        let mut providers = Vec::new();
        for index in 0..files.top_level as usize {
            let item = NSPasteboardItem::new();
            let provider = provider::Provider::for_file(
                generation,
                files.clone(),
                index,
                Arc::clone(&published),
                self.pastes.clone(),
            );
            item.setDataProvider_forTypes(
                ProtocolObject::from_ref(&*provider),
                &NSArray::from_slice(&[file_url_type()]),
            );
            items.push(item);
            providers.push(provider);
        }
        (items, providers)
    }

    /// Paths of the file URLs on the pasteboard; empty when it holds none.
    fn file_paths(&self) -> Vec<PathBuf> {
        // SAFETY: The key is an immutable AppKit global.
        let key = unsafe { NSPasteboardURLReadingFileURLsOnlyKey };
        let yes = NSNumber::new_bool(true);
        let options: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[key], &[&**yes]);
        let classes = NSArray::from_slice(&[NSURL::class()]);
        // SAFETY: The classes array holds NSURL and the only option is a
        // documented boolean for that class.
        let urls = unsafe {
            self.board
                .readObjectsForClasses_options(&classes, Some(&options))
        };
        let Some(urls) = urls else {
            return Vec::new();
        };
        // More entries than a manifest may hold can never be sent; read none.
        if urls.count() > limits::MANIFEST_ENTRIES_MAX {
            return Vec::new();
        }
        (0..urls.count())
            .map(|index| urls.objectAtIndex(index))
            .filter_map(|url| url.downcast::<NSURL>().ok())
            .filter_map(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
            .collect()
    }
}

fn finish_providers(providers: &mut Vec<Retained<provider::Provider>>) {
    for provider in providers.drain(..) {
        provider.finish();
    }
}

fn file_url_type() -> &'static NSString {
    ns_string!("public.file-url")
}

fn formats() -> [(&'static str, &'static NSString); 3] {
    [
        (TEXT_PLAIN, ns_string!("public.utf8-plain-text")),
        (TEXT_HTML, ns_string!("public.html")),
        (IMAGE_PNG, ns_string!("public.png")),
    ]
}

fn uti(mime: &str) -> Option<&'static NSString> {
    formats()
        .into_iter()
        .find_map(|(supported, uti)| (mime == supported).then_some(uti))
}

fn convert_image(data: &NSData, kind: NSBitmapImageFileType) -> Option<Retained<NSData>> {
    if data.length() > limits::IMAGE_DECODED_MAX {
        return None;
    }
    let image = NSBitmapImageRep::imageRepWithData(data)?;
    let width = u64::try_from(image.pixelsWide()).ok()?;
    let height = u64::try_from(image.pixelsHigh()).ok()?;
    if width == 0
        || height == 0
        || width > u64::from(limits::IMAGE_MAX_DIMENSION)
        || height > u64::from(limits::IMAGE_MAX_DIMENSION)
        || width * height * 4 > limits::IMAGE_DECODED_MAX as u64
    {
        return None;
    }
    // SAFETY: An empty property dictionary supplies no incorrectly typed encoder options.
    unsafe { image.representationUsingType_properties(kind, &NSDictionary::new()) }
}
