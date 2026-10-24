// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Mutex;

use a7n_core::clipboard::{Generation, IMAGE_PNG, limit_for, validate_png};
use objc2::rc::Retained;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::{
    NSBitmapImageFileType, NSPasteboard, NSPasteboardItem, NSPasteboardItemDataProvider,
};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSString, ns_string};
use tokio::sync::mpsc;

use crate::clipboard::{LazyItem, PasteRequest};

pub(super) struct Offer {
    generation: Generation,
    items: Vec<LazyItem>,
    pastes: mpsc::Sender<PasteRequest>,
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
            let request = self.ivars().lock().ok().and_then(|state| state.as_ref().and_then(|offer| {
                let entry = offer.items.iter().find(|entry| super::uti(&entry.mime) == Some(kind) || (entry.mime == IMAGE_PNG && kind == ns_string!("public.tiff")))?;
                Some((offer.generation, entry.clone(), offer.pastes.clone()))
            }));
            let Some((generation, entry, pastes)) = request else { return };
            if let Some(bytes) = fetch(generation, &entry, &pastes) {
                let data = NSData::with_bytes(&bytes);
                if entry.mime == IMAGE_PNG {
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

impl Provider {
    pub(super) fn new(
        generation: Generation,
        items: Vec<LazyItem>,
        pastes: mpsc::Sender<PasteRequest>,
    ) -> Retained<Self> {
        let allocated = Self::alloc().set_ivars(Mutex::new(Some(Offer {
            generation,
            items,
            pastes,
        })));
        // SAFETY: NSObject's init initializes the allocated superclass exactly once.
        unsafe { msg_send![super(allocated), init] }
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
    let bytes = crate::clipboard::fetch::fetch(generation, item.item_index, pastes)?;
    (bytes.len() <= limit_for(&item.mime) && bytes.len() as u64 == item.size).then_some(bytes)
}
