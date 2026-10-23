// SPDX-License-Identifier: GPL-3.0-or-later

//! Inject side: libei sender devices of the `RemoteDesktop` session.
//!
//! The injector cannot ask the compositor where the real cursor is, so it
//! tracks the position itself: it starts in the middle of the screens, follows
//! every motion and warp, and is clamped to the bounds. On screens that are
//! not one rectangle the tracked position can drift from the real cursor.

use std::time::Duration;

use a7n_proto::v1::{ButtonEvent, InputEvent, KeyEvent, ScrollEvent, input_event};
use futures_util::StreamExt;
use reis::ei;
use reis::event::{Device, DeviceCapability, EiEvent};
use rustix::time::{ClockId, clock_gettime};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{Receiver, Sender};

use super::buttons::evdev_from_button;
use super::scroll::{ScrollRequest, scroll_request};
use super::session::{DesktopSession, Eis};
use super::zones;
use crate::input::evdev_keymap::evdev_from_hid;
use crate::input::{Bounds, Captured, Inject, InputError};

/// How long the compositor gets to present a resumed keyboard and pointer
/// after the session started.
const DEVICE_SETUP_TIMEOUT: Duration = Duration::from_secs(5);
const MICROS_PER_SECOND: u64 = 1_000_000;
const NANOS_PER_MICRO: u64 = 1_000;

struct Slot {
    device: Device,
    resumed: bool,
    emulating: bool,
}

pub(super) struct Injecting {
    session: DesktopSession,
    slots: Vec<Slot>,
    sequence: u32,
    bounds: Bounds,
    position: (i32, i32),
}

impl Injecting {
    /// Binds the seat and waits until a keyboard and a pointer can be driven.
    pub(super) async fn new(session: DesktopSession, bounds: Bounds) -> Result<Self, InputError> {
        let mut injecting = Self {
            session,
            slots: Vec::new(),
            sequence: 0,
            position: center(bounds),
            bounds,
        };
        let ready = tokio::time::timeout(DEVICE_SETUP_TIMEOUT, injecting.wait_for_devices());
        match ready.await {
            Ok(result) => result?,
            Err(_) => {
                return Err(InputError::Portal(
                    "the compositor offered no keyboard and pointer to inject into".to_owned(),
                ));
            }
        }
        Ok(injecting)
    }

    async fn wait_for_devices(&mut self) -> Result<(), InputError> {
        while !(self.can_drive(DeviceCapability::Keyboard)
            && self.can_drive(DeviceCapability::Pointer))
        {
            self.next_event().await?;
        }
        Ok(())
    }

    /// Runs until the daemon goes away or the compositor ends the session.
    pub(super) async fn run(
        &mut self,
        commands: &mut Receiver<Inject>,
        pointer: &Sender<Captured>,
    ) -> Result<(), InputError> {
        loop {
            tokio::select! {
                result = self.next_event() => result?,
                command = commands.recv() => {
                    let Some(command) = command else {
                        return Ok(());
                    };
                    let report = self.apply(command);
                    self.session.eis.connection.flush().map_err(|error| {
                        InputError::Portal(format!("cannot write to the EI socket: {error}"))
                    })?;
                    if let Some(report) = report {
                        // Never block: a superseded position report is dropped.
                        if let Err(TrySendError::Closed(_)) = pointer.try_send(report) {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// The clipboard service of the session, to be run by the backend thread.
    pub(super) fn take_clipboard(&mut self) -> Option<crate::clipboard::wayland::portal::Service> {
        self.session.clipboard.take()
    }

    /// Ends the emulation and closes the session; best effort.
    pub(super) async fn shutdown(&mut self) {
        let serial = self.session.eis.connection.serial();
        for slot in self.slots.iter_mut().filter(|slot| slot.emulating) {
            slot.device.device().stop_emulating(serial);
            slot.emulating = false;
        }
        let _ = self.session.eis.connection.flush();
        let _ = self.session.session.close().await;
    }

    async fn next_event(&mut self) -> Result<(), InputError> {
        let Eis { events, .. } = &mut self.session.eis;
        match events.next().await {
            Some(Ok(event)) => self.on_ei(&event),
            Some(Err(error)) => Err(InputError::Portal(format!("libei failed: {error}"))),
            None => Err(InputError::Portal(
                "the compositor closed the injection stream".to_owned(),
            )),
        }
    }

    fn on_ei(&mut self, event: &EiEvent) -> Result<(), InputError> {
        match event {
            EiEvent::SeatAdded(added) => {
                added.seat.bind_capabilities(
                    DeviceCapability::Pointer
                        | DeviceCapability::PointerAbsolute
                        | DeviceCapability::Keyboard
                        | DeviceCapability::Button
                        | DeviceCapability::Scroll,
                );
                self.session.eis.connection.flush().map_err(|error| {
                    InputError::Portal(format!("cannot write to the EI socket: {error}"))
                })?;
            }
            EiEvent::DeviceAdded(added) => self.slots.push(Slot {
                device: added.device.clone(),
                resumed: false,
                emulating: false,
            }),
            EiEvent::DeviceRemoved(removed) => {
                self.slots.retain(|slot| slot.device != removed.device);
            }
            EiEvent::DeviceResumed(resumed) => self.set_resumed(&resumed.device, true),
            EiEvent::DevicePaused(paused) => self.set_resumed(&paused.device, false),
            EiEvent::Disconnected(gone) => {
                let reason = gone.explanation.as_deref().unwrap_or("no reason given");
                return Err(InputError::Portal(format!(
                    "the compositor dropped the injection stream: {reason}"
                )));
            }
            _ => {}
        }
        Ok(())
    }

    fn set_resumed(&mut self, device: &Device, resumed: bool) {
        for slot in self.slots.iter_mut().filter(|slot| &slot.device == device) {
            slot.resumed = resumed;
            // A pause ends the emulation sequence on the server's side too.
            slot.emulating &= resumed;
        }
    }

    fn can_drive(&self, capability: DeviceCapability) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.resumed && slot.device.has_capability(capability))
    }

    /// Performs one command and returns the pointer report it earns, if any.
    fn apply(&mut self, command: Inject) -> Option<Captured> {
        match command {
            Inject::Event(event) => {
                self.event(&event);
                None
            }
            Inject::Motion { dx, dy } => {
                self.motion(dx, dy);
                Some(self.report(dx, dy))
            }
            Inject::Warp { x, y } => {
                self.warp(x, y);
                Some(self.report(0, 0))
            }
        }
    }

    fn report(&self, dx: i32, dy: i32) -> Captured {
        Captured::Pointer {
            x: self.position.0,
            y: self.position.1,
            dx,
            dy,
        }
    }

    fn event(&mut self, event: &InputEvent) {
        // Text and gestures have no libei equivalent here: `ei_text` needs
        // libei 1.6 and a compositor that offers it, and gestures are a
        // client-side toolkit concept. Both are dropped, as on X11.
        match &event.event {
            Some(input_event::Event::Key(key)) => self.key(key),
            Some(input_event::Event::Button(button)) => self.button(*button),
            Some(input_event::Event::Scroll(scroll)) => self.scroll(scroll),
            Some(input_event::Event::Gesture(_) | input_event::Event::Text(_)) | None => {}
        }
    }

    fn key(&mut self, key: &KeyEvent) {
        let Some(code) = evdev_from_hid(key.usage) else {
            return;
        };
        let state = if key.down {
            ei::keyboard::KeyState::Press
        } else {
            ei::keyboard::KeyState::Released
        };
        self.send(DeviceCapability::Keyboard, |device| {
            if let Some(keyboard) = device.interface::<ei::Keyboard>() {
                keyboard.key(u32::from(code), state);
            }
        });
    }

    fn button(&mut self, event: ButtonEvent) {
        let Some(code) = evdev_from_button(event.button()) else {
            return;
        };
        let state = if event.down {
            ei::button::ButtonState::Press
        } else {
            ei::button::ButtonState::Released
        };
        self.send(DeviceCapability::Button, |device| {
            if let Some(button) = device.interface::<ei::Button>() {
                button.button(code, state);
            }
        });
    }

    fn scroll(&mut self, event: &ScrollEvent) {
        let Some(request) = scroll_request(event) else {
            return;
        };
        self.send(DeviceCapability::Scroll, |device| {
            let Some(scroll) = device.interface::<ei::Scroll>() else {
                return;
            };
            match request {
                ScrollRequest::Discrete { x, y } => scroll.scroll_discrete(x, y),
                ScrollRequest::Smooth { x, y } => scroll.scroll(x, y),
            }
        });
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "pixel deltas are far below 2^24"
    )]
    fn motion(&mut self, dx: i32, dy: i32) {
        let sent = self.send(DeviceCapability::Pointer, |device| {
            if let Some(pointer) = device.interface::<ei::Pointer>() {
                pointer.motion_relative(dx as f32, dy as f32);
            }
        });
        if sent {
            self.position = self.clamped(
                self.position.0.saturating_add(dx),
                self.position.1.saturating_add(dy),
            );
        }
    }

    /// Without an absolute pointer the cursor cannot be placed, so the warp is
    /// ignored and the tracked position stays where the real cursor is.
    #[allow(
        clippy::cast_precision_loss,
        reason = "screen coordinates are far below 2^24"
    )]
    fn warp(&mut self, x: i32, y: i32) {
        let Some(slot) = self.absolute_slot() else {
            return;
        };
        let regions = region_bounds(&slot.device).unwrap_or_else(|| vec![self.bounds]);
        let Some((x, y)) = zones::nearest_inside(&regions, x, y) else {
            return;
        };
        let sent = self.send(DeviceCapability::PointerAbsolute, |device| {
            if let Some(pointer) = device.interface::<ei::PointerAbsolute>() {
                pointer.motion_absolute(x as f32, y as f32);
            }
        });
        if sent {
            self.position = self.clamped(x, y);
        }
    }

    fn absolute_slot(&self) -> Option<&Slot> {
        self.slots.iter().find(|slot| {
            slot.resumed
                && slot
                    .device
                    .has_capability(DeviceCapability::PointerAbsolute)
        })
    }

    fn clamped(&self, x: i32, y: i32) -> (i32, i32) {
        zones::nearest_inside(&[self.bounds], x, y).unwrap_or((x, y))
    }

    /// Sends one frame to the first resumed device with `capability`, opening
    /// its emulation sequence first if needed. Returns whether a device took it.
    fn send(&mut self, capability: DeviceCapability, action: impl FnOnce(&Device)) -> bool {
        let serial = self.session.eis.connection.serial();
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.resumed && slot.device.has_capability(capability))
        else {
            return false;
        };
        if !slot.emulating {
            slot.device.device().start_emulating(serial, self.sequence);
            self.sequence = self.sequence.wrapping_add(1);
            slot.emulating = true;
        }
        action(&slot.device);
        slot.device.device().frame(serial, now_micros());
        true
    }
}

fn center(bounds: Bounds) -> (i32, i32) {
    let half = |length: u32| i32::try_from(length / 2).unwrap_or(i32::MAX);
    (
        bounds.x.saturating_add(half(bounds.width)),
        bounds.y.saturating_add(half(bounds.height)),
    )
}

fn region_bounds(device: &Device) -> Option<Vec<Bounds>> {
    let regions: Vec<Bounds> = device
        .regions()
        .iter()
        .filter_map(|region| {
            Some(Bounds {
                x: i32::try_from(region.x).ok()?,
                y: i32::try_from(region.y).ok()?,
                width: region.width,
                height: region.height,
            })
        })
        .collect();
    (!regions.is_empty()).then_some(regions)
}

/// Monotonic microseconds, the clock libei timestamps use.
fn now_micros() -> u64 {
    let now = clock_gettime(ClockId::Monotonic);
    let seconds = u64::try_from(now.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(now.tv_nsec).unwrap_or(0);
    seconds.saturating_mul(MICROS_PER_SECOND) + nanos / NANOS_PER_MICRO
}
