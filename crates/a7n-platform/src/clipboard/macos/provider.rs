// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use a7n_core::clipboard::Generation;
use a7n_proto::limits::{CLIPBOARD_FETCH_TIMEOUT, CLIPBOARD_TEXT_MAX};
use objc2::rc::Retained;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardItemDataProvider};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSString};
use tokio::sync::{mpsc, oneshot};

use crate::clipboard::{LazyItem, PasteRequest};

const REPLY_INTERVAL: Duration = Duration::from_millis(10);

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
                let entry = offer.items.iter().find(|entry| super::uti(&entry.mime) == Some(kind))?;
                Some((offer.generation, entry.clone(), offer.pastes.clone()))
            }));
            let Some((generation, entry, pastes)) = request else { return };
            if let Some(bytes) = fetch(generation, &entry, &pastes) {
                item.setData_forType(&NSData::with_bytes(&bytes), kind);
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
    let (reply, mut receiver) = oneshot::channel();
    pastes
        .try_send(PasteRequest {
            generation,
            item_index: item.item_index,
            reply,
        })
        .ok()?;
    let deadline = Instant::now() + CLIPBOARD_FETCH_TIMEOUT;
    loop {
        match receiver.try_recv() {
            Ok(Some(bytes))
                if bytes.len() <= CLIPBOARD_TEXT_MAX && bytes.len() as u64 == item.size =>
            {
                return Some(bytes);
            }
            Err(oneshot::error::TryRecvError::Empty) if Instant::now() < deadline => {
                thread::sleep(
                    REPLY_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            _ => return None,
        }
    }
}
