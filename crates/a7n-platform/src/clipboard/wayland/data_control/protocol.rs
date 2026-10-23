// SPDX-License-Identifier: GPL-3.0-or-later

//! The two data-control protocols behind one set of types.
//!
//! `ext_data_control_v1` and `zwlr_data_control_unstable_v1` have the same
//! requests and events; the enums only pick the generated type.

use std::os::fd::BorrowedFd;

use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

use super::State;

const EXT_MANAGER: &str = "ext_data_control_manager_v1";
const WLR_MANAGER: &str = "zwlr_data_control_manager_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Protocol {
    Ext,
    Wlr,
}

impl Protocol {
    /// The standardised protocol wins over the wlroots one.
    pub(super) fn pick(interfaces: &[&str]) -> Option<Self> {
        let has = |name: &str| interfaces.contains(&name);
        if has(EXT_MANAGER) {
            Some(Self::Ext)
        } else if has(WLR_MANAGER) {
            Some(Self::Wlr)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub(super) enum Manager {
    Ext(ExtDataControlManagerV1),
    Wlr(ZwlrDataControlManagerV1),
}

#[derive(Debug)]
pub(super) enum Device {
    Ext(ExtDataControlDeviceV1),
    Wlr(ZwlrDataControlDeviceV1),
}

#[derive(Debug)]
pub(super) enum Source {
    Ext(ExtDataControlSourceV1),
    Wlr(ZwlrDataControlSourceV1),
}

#[derive(Debug)]
pub(super) enum Offer {
    Ext(ExtDataControlOfferV1),
    Wlr(ZwlrDataControlOfferV1),
}

impl Manager {
    pub(super) fn device(&self, seat: &WlSeat, qh: &QueueHandle<State>) -> Device {
        match self {
            Self::Ext(manager) => Device::Ext(manager.get_data_device(seat, qh, ())),
            Self::Wlr(manager) => Device::Wlr(manager.get_data_device(seat, qh, ())),
        }
    }

    pub(super) fn source(&self, qh: &QueueHandle<State>) -> Source {
        match self {
            Self::Ext(manager) => Source::Ext(manager.create_data_source(qh, ())),
            Self::Wlr(manager) => Source::Wlr(manager.create_data_source(qh, ())),
        }
    }
}

impl Device {
    pub(super) fn set_selection(&self, source: &Source) {
        match (self, source) {
            (Self::Ext(device), Source::Ext(source)) => device.set_selection(Some(source)),
            (Self::Wlr(device), Source::Wlr(source)) => device.set_selection(Some(source)),
            // Both come from the same manager.
            _ => {}
        }
    }
}

impl Source {
    pub(super) fn id(&self) -> ObjectId {
        match self {
            Self::Ext(source) => source.id(),
            Self::Wlr(source) => source.id(),
        }
    }

    pub(super) fn offer(&self, mime: &str) {
        match self {
            Self::Ext(source) => source.offer(mime.to_owned()),
            Self::Wlr(source) => source.offer(mime.to_owned()),
        }
    }

    pub(super) fn destroy(&self) {
        match self {
            Self::Ext(source) => source.destroy(),
            Self::Wlr(source) => source.destroy(),
        }
    }
}

impl Offer {
    pub(super) fn id(&self) -> ObjectId {
        match self {
            Self::Ext(offer) => offer.id(),
            Self::Wlr(offer) => offer.id(),
        }
    }

    pub(super) fn receive(&self, mime: &str, fd: BorrowedFd<'_>) {
        match self {
            Self::Ext(offer) => offer.receive(mime.to_owned(), fd),
            Self::Wlr(offer) => offer.receive(mime.to_owned(), fd),
        }
    }

    pub(super) fn destroy(&self) {
        match self {
            Self::Ext(offer) => offer.destroy(),
            Self::Wlr(offer) => offer.destroy(),
        }
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtDataControlManagerV1,
        _: <ExtDataControlManagerV1 as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_data_control_device_v1::Event;
        match event {
            Event::DataOffer { id } => state.on_data_offer(&Offer::Ext(id)),
            Event::Selection { id } => state.on_selection(id.map(Offer::Ext)),
            Event::PrimarySelection { id } => state.on_other_selection(id.map(Offer::Ext)),
            Event::Finished => state.on_finished(),
            _ => {}
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_data_control_device_v1::Event;
        match event {
            Event::DataOffer { id } => state.on_data_offer(&Offer::Wlr(id)),
            Event::Selection { id } => state.on_selection(id.map(Offer::Wlr)),
            Event::PrimarySelection { id } => state.on_other_selection(id.map(Offer::Wlr)),
            Event::Finished => state.on_finished(),
            _ => {}
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        offer: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            state.on_offered_mime(&offer.id(), mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            state.on_offered_mime(&offer.id(), mime_type);
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                state.on_send(&source.id(), &mime_type, fd);
            }
            ext_data_control_source_v1::Event::Cancelled => {
                state.on_cancelled(&Source::Ext(source.clone()));
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                state.on_send(&source.id(), &mime_type, fd);
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                state.on_cancelled(&Source::Wlr(source.clone()));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_both_protocols_offered_prefers_ext() {
        // Arrange
        let cases = [
            (
                vec![WLR_MANAGER, EXT_MANAGER, "wl_seat"],
                Some(Protocol::Ext),
            ),
            (vec!["wl_seat", WLR_MANAGER], Some(Protocol::Wlr)),
            (vec!["wl_seat", "wl_data_device_manager"], None),
        ];
        for (interfaces, expected) in cases {
            // Act
            let picked = Protocol::pick(&interfaces);

            // Assert
            assert_eq!(picked, expected);
        }
    }
}
