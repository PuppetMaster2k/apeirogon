// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use a7n_core::clipboard::{FILES, Generation, IMAGE_PNG, validate_png};
use a7n_proto::limits;
use objc2::rc::Retained;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::{
    NSBitmapImageFileType, NSPasteboard, NSPasteboardItem, NSPasteboardItemDataProvider,
};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSString, NSURL, ns_string};
use tokio::sync::mpsc;

use crate::clipboard::{FILE_PASTE_TIMEOUT, LazyItem, PasteRequest, uri_list};

/// The published paths of one file paste, shared by the items of a copy so
/// the daemon is asked once however many of them the receiver reads.
pub(super) type PublishedFiles = Arc<Mutex<Option<Vec<PathBuf>>>>;

/// Which pasteboard item of a file list a provider answers for.
struct FileSlot {
    index: usize,
    published: PublishedFiles,
}

pub(super) struct Offer {
    generation: Generation,
    items: Vec<LazyItem>,
    pastes: mpsc::Sender<PasteRequest>,
    file: Option<FileSlot>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop implementation.
    #[unsafe(super = NSObject)]
    #[name = "A7nClipboardDataProvider"]
    #[ivars = Mutex<Option<Offer>>]
    pub(super) struct Provider;

    // SAFETY: NSObjectProtocol adds no requirements.
    unsafe impl NSObjectProtocol for Provider {}

    // SAFETY: Methods implement the protocol with its declared signatures.
    unsafe impl NSPasteboardItemDataProvider for Provider {
        #[unsafe(method(pasteboard:item:provideDataForType:))]
        fn provide(&self, _board: Option<&NSPasteboard>, item: &NSPasteboardItem, kind: &NSString) {
            let Some(request) = self.request(kind) else { return };
            if let Some(slot) = request.file {
                provide_file(&request.entry, request.generation, &request.pastes, &slot, item, kind);
                return;
            }
            if let Some(bytes) = fetch(request.generation, &request.entry, &request.pastes) {
                let data = NSData::with_bytes(&bytes);
                if request.entry.mime == IMAGE_PNG {
                    if validate_png(&bytes).is_err() { return; }
                    if kind == ns_string!("public.tiff") {
                        if let Some(tiff) = super::convert_image(&data, NSBitmapImageFileType::TIFF) {
                            item.setData_forType(&tiff, kind);
                        }
                        return;
                    }
                }
                item.setData_forType(&data, kind);
            }
        }

        #[unsafe(method(pasteboardFinishedWithDataProvider:))]
        fn finished(&self, _board: &NSPasteboard) {
            self.finish();
        }
    }
);

/// What one callback needs, copied out so no lock is held while it blocks.
struct Request {
    generation: Generation,
    entry: LazyItem,
    pastes: mpsc::Sender<PasteRequest>,
    file: Option<(usize, PublishedFiles)>,
}

impl Provider {
    pub(super) fn new(
        generation: Generation,
        items: Vec<LazyItem>,
        pastes: mpsc::Sender<PasteRequest>,
    ) -> Retained<Self> {
        Self::with_offer(Offer {
            generation,
            items,
            pastes,
            file: None,
        })
    }

    /// Answers for the `index`-th top-level entry of a published file list.
    pub(super) fn for_file(
        generation: Generation,
        files: LazyItem,
        index: usize,
        published: PublishedFiles,
        pastes: mpsc::Sender<PasteRequest>,
    ) -> Retained<Self> {
        Self::with_offer(Offer {
            generation,
            items: vec![files],
            pastes,
            file: Some(FileSlot { index, published }),
        })
    }

    fn with_offer(offer: Offer) -> Retained<Self> {
        let allocated = Self::alloc().set_ivars(Mutex::new(Some(offer)));
        // SAFETY: NSObject's init initializes the allocated superclass exactly once.
        unsafe { msg_send![super(allocated), init] }
    }

    fn request(&self, kind: &NSString) -> Option<Request> {
        let state = self.ivars().lock().ok()?;
        let offer = state.as_ref()?;
        let entry = offer.items.iter().find(|entry| {
            super::uti(&entry.mime) == Some(kind)
                || (entry.mime == IMAGE_PNG && kind == ns_string!("public.tiff"))
                || (entry.mime == FILES && kind == super::file_url_type())
        })?;
        Some(Request {
            generation: offer.generation,
            entry: entry.clone(),
            pastes: offer.pastes.clone(),
            file: offer
                .file
                .as_ref()
                .map(|slot| (slot.index, Arc::clone(&slot.published))),
        })
    }

    pub(super) fn finish(&self) {
        if let Ok(mut state) = self.ivars().lock() {
            state.take();
        }
    }
}

fn fetch(
    generation: Generation,
    item: &LazyItem,
    pastes: &mpsc::Sender<PasteRequest>,
) -> Option<Vec<u8>> {
    let bytes = crate::clipboard::fetch::fetch(
        generation,
        item.item_index,
        limits::CLIPBOARD_FETCH_TIMEOUT,
        pastes,
    )?;
    (bytes.len() <= a7n_core::clipboard::limit_for(&item.mime) && bytes.len() as u64 == item.size)
        .then_some(bytes)
}

fn provide_file(
    entry: &LazyItem,
    generation: Generation,
    pastes: &mpsc::Sender<PasteRequest>,
    (index, published): &(usize, PublishedFiles),
    item: &NSPasteboardItem,
    kind: &NSString,
) {
    let Some(path) = published_path(entry, generation, pastes, *index, published) else {
        return;
    };
    let Some(text) = path.to_str() else { return };
    let url = NSURL::fileURLWithPath(&NSString::from_str(text));
    let Some(url) = url.absoluteString() else {
        return;
    };
    item.setData_forType(&NSData::with_bytes(url.to_string().as_bytes()), kind);
}

/// Files are fetched only by the first item asked; the others reuse its paths.
fn published_path(
    entry: &LazyItem,
    generation: Generation,
    pastes: &mpsc::Sender<PasteRequest>,
    index: usize,
    published: &PublishedFiles,
) -> Option<PathBuf> {
    let known = published.lock().ok()?.clone();
    if let Some(paths) = known {
        return paths.get(index).cloned();
    }
    let paths = fetch_paths(entry, generation, pastes)?;
    let path = paths.get(index).cloned();
    *published.lock().ok()? = Some(paths);
    path
}

fn fetch_paths(
    entry: &LazyItem,
    generation: Generation,
    pastes: &mpsc::Sender<PasteRequest>,
) -> Option<Vec<PathBuf>> {
    // The reply arrives only after the files are downloaded, and a file list
    // is bounded by policy, not by the clipboard limits.
    let reply =
        crate::clipboard::fetch::fetch(generation, entry.item_index, FILE_PASTE_TIMEOUT, pastes)?;
    Some(uri_list::decode(&reply))
}
