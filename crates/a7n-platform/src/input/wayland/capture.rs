// SPDX-License-Identifier: GPL-3.0-or-later

//! Capture side: pointer barriers at the outer edge of the screens, captured
//! libei events forwarded to the daemon, and the release rules of
//! [`CaptureFlow`].

use std::num::NonZeroU32;
use std::pin::pin;
use std::time::{Duration, Instant};

use ashpd::desktop::input_capture::{
    Activated, Barrier, DisableOptions, EnableOptions, GetZonesOptions, InputCapture,
    ReleaseOptions, SetPointerBarriersOptions,
};
use futures_util::StreamExt;
use reis::ei;
use reis::event::{DeviceCapability, EiEvent};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{Receiver, Sender};

use super::release::{CaptureFlow, ReleaseOrder};
use super::session::{CaptureSession, portal_error};
use super::translate::{Raw, Translated, Translator};
use super::zones;
use crate::input::{Bounds, CaptureControl, Captured, InputError};

/// Longest wait before the release rules and the dead-man switch are looked at
/// again when nothing else happens.
const WAKE_INTERVAL: Duration = Duration::from_millis(50);
const CAPTURE_PERMISSION: &str = "Wayland input capture";

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Continue,
    /// The daemon dropped its end of the channels.
    Stop,
}

pub(super) struct Capturer {
    session: CaptureSession,
    flow: CaptureFlow,
    translator: Translator,
    bounds: Bounds,
}

impl Capturer {
    /// Learns the zones and places the barriers. Capture is armed when
    /// [`Capturer::run`] starts.
    pub(super) async fn new(session: CaptureSession) -> Result<Self, InputError> {
        let mut capturer = Self {
            session,
            flow: CaptureFlow::new(Vec::new(), Instant::now()),
            translator: Translator::default(),
            bounds: Bounds {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
        };
        capturer.bounds = capturer.place_barriers().await?;
        Ok(capturer)
    }

    /// Union of the zones when the session started. Later zone changes re-arm
    /// the barriers but cannot change what the daemon was told.
    pub(super) fn bounds(&self) -> Bounds {
        self.bounds
    }

    /// Runs until the daemon goes away or the compositor ends the session.
    pub(super) async fn run(
        &mut self,
        events: &Sender<Captured>,
        control: &mut Receiver<CaptureControl>,
    ) -> Result<(), InputError> {
        let fail = |error| portal_error(CAPTURE_PERMISSION, error);
        // Subscribed before `Enable` so that an immediate push is not missed.
        // Signals come from a second proxy because their streams borrow it,
        // while the session's own proxy is needed for method calls meanwhile.
        let portal = InputCapture::new().await.map_err(fail)?;
        let mut activated = pin!(portal.receive_activated().await.map_err(fail)?);
        let mut deactivated = pin!(portal.receive_deactivated().await.map_err(fail)?);
        let mut disabled = pin!(portal.receive_disabled().await.map_err(fail)?);
        let mut zones_changed = pin!(portal.receive_zones_changed().await.map_err(fail)?);
        self.enable().await?;
        let mut tick = tokio::time::interval(WAKE_INTERVAL);
        loop {
            let mut order = None;
            let step = tokio::select! {
                event = self.session.eis.events.next() => match event {
                    Some(Ok(event)) => self.on_ei(&event, events)?,
                    Some(Err(error)) => return Err(ei_error(&error)),
                    None => return Err(InputError::Portal("the compositor closed the input stream".to_owned())),
                },
                Some(activated) = activated.next() => {
                    self.on_activated(&activated);
                    Step::Continue
                }
                Some(deactivated) = deactivated.next() => {
                    self.flow.deactivate(deactivated.activation_id().unwrap_or_default());
                    Step::Continue
                }
                Some(_) = disabled.next() => {
                    self.enable().await?;
                    Step::Continue
                }
                Some(_) = zones_changed.next() => {
                    self.place_barriers().await?;
                    self.enable().await?;
                    Step::Continue
                }
                command = control.recv() => match command {
                    Some(command) => {
                        order = self.flow.control(command, Instant::now());
                        Step::Continue
                    }
                    None => Step::Stop,
                },
                _ = tick.tick() => Step::Continue,
            };
            if step == Step::Stop {
                return Ok(());
            }
            if let Some(order) = order.or_else(|| self.flow.poll(Instant::now())) {
                self.release(order).await?;
            }
        }
    }

    /// Gives any running capture back and closes the session. Best effort: if
    /// the bus is gone, the compositor ends the capture with the connection.
    pub(super) async fn shutdown(&mut self) {
        if let Some(order) = self.flow.release() {
            let _ = self.release_capture(order).await;
        }
        let _ = self
            .session
            .portal
            .disable(&self.session.session, DisableOptions::default())
            .await;
        let _ = self.session.session.close().await;
    }

    fn on_activated(&mut self, activated: &Activated) {
        let id = activated.activation_id().unwrap_or_default();
        // Without a position the push cannot be placed; the middle of the
        // screens is the least wrong guess.
        let cursor = activated.cursor_position().map_or_else(
            || {
                let bounds = self.bounds;
                (
                    f64::from(bounds.x) + f64::from(bounds.width) / 2.0,
                    f64::from(bounds.y) + f64::from(bounds.height) / 2.0,
                )
            },
            |(x, y)| (f64::from(x), f64::from(y)),
        );
        self.translator = Translator::default();
        self.flow.activate(id, cursor, Instant::now());
    }

    fn on_ei(&mut self, event: &EiEvent, events: &Sender<Captured>) -> Result<Step, InputError> {
        match event {
            EiEvent::SeatAdded(added) => {
                added.seat.bind_capabilities(
                    DeviceCapability::Pointer
                        | DeviceCapability::Keyboard
                        | DeviceCapability::Button
                        | DeviceCapability::Scroll,
                );
                self.session.eis.connection.flush().map_err(flush_error)?;
                return Ok(Step::Continue);
            }
            EiEvent::Disconnected(gone) => {
                let reason = gone.explanation.as_deref().unwrap_or("no reason given");
                return Err(InputError::Portal(format!(
                    "the compositor dropped the input stream: {reason}"
                )));
            }
            _ => {}
        }
        // Events that raced ahead of the `Activated` signal, or arrive after
        // a release, belong to no capture.
        if !self.flow.is_active() {
            return Ok(Step::Continue);
        }
        let Some(translated) = raw_event(event).and_then(|raw| self.translator.push(raw)) else {
            return Ok(Step::Continue);
        };
        let captured = match translated {
            Translated::Motion { dx, dy } => {
                let Some((x, y)) = self.flow.motion(dx, dy, Instant::now()) else {
                    return Ok(Step::Continue);
                };
                Captured::Pointer { x, y, dx, dy }
            }
            Translated::Input(input) => Captured::Input(input),
        };
        // Never block: a stalled daemon must not keep input captured, so a
        // full queue drops the event and the dead-man switch gets its turn.
        match events.try_send(captured) {
            Err(TrySendError::Closed(_)) => Ok(Step::Stop),
            Ok(()) | Err(TrySendError::Full(_)) => Ok(Step::Continue),
        }
    }

    async fn release(&mut self, order: ReleaseOrder) -> Result<(), InputError> {
        self.release_capture(order).await?;
        // Make sure the barriers are armed again. A compositor that stays
        // enabled across a release may refuse a second `Enable`; that is fine.
        if self.enable().await.is_err() {
            eprintln!("kind=input_wayland_reenable_failed");
        }
        Ok(())
    }

    async fn release_capture(&self, order: ReleaseOrder) -> Result<(), InputError> {
        let options = ReleaseOptions::default()
            .set_activation_id(order.activation_id)
            .set_cursor_position(order.cursor);
        self.session
            .portal
            .release(&self.session.session, options)
            .await
            .map_err(|error| portal_error(CAPTURE_PERMISSION, error))
    }

    async fn enable(&self) -> Result<(), InputError> {
        self.session
            .portal
            .enable(&self.session.session, EnableOptions::default())
            .await
            .map_err(|error| portal_error(CAPTURE_PERMISSION, error))
    }

    /// Reads the zones and sets barriers on their outer edge, which suspends
    /// the session until the next `Enable`. Returns the bounds of the zones.
    async fn place_barriers(&mut self) -> Result<Bounds, InputError> {
        let fail = |error| portal_error(CAPTURE_PERMISSION, error);
        let reply = self
            .session
            .portal
            .zones(&self.session.session, GetZonesOptions::default())
            .await
            .and_then(|request| request.response())
            .map_err(fail)?;
        let zone_list: Vec<Bounds> = reply
            .regions()
            .iter()
            .map(|region| Bounds {
                x: region.x_offset(),
                y: region.y_offset(),
                width: region.width(),
                height: region.height(),
            })
            .collect();
        let bounds = zones::bounding_box(&zone_list)
            .ok_or_else(|| InputError::Portal("the compositor reported no screens".to_owned()))?;
        let barriers = portal_barriers(&zone_list);
        let outcome = self
            .session
            .portal
            .set_pointer_barriers(
                &self.session.session,
                &barriers,
                reply.zone_set(),
                SetPointerBarriersOptions::default(),
            )
            .await
            .and_then(|request| request.response())
            .map_err(fail)?;
        if outcome.failed_barriers().len() >= barriers.len() {
            return Err(InputError::Portal(
                "the compositor refused every pointer barrier".to_owned(),
            ));
        }
        self.flow.set_zones(zone_list);
        Ok(bounds)
    }
}

fn portal_barriers(zone_list: &[Bounds]) -> Vec<Barrier> {
    (1_u32..)
        .zip(zones::outer_barriers(zone_list))
        .filter_map(|(id, barrier)| {
            Some(Barrier::new(
                NonZeroU32::new(id)?,
                (barrier.x1, barrier.y1, barrier.x2, barrier.y2),
            ))
        })
        .collect()
}

fn raw_event(event: &EiEvent) -> Option<Raw> {
    let raw = match event {
        EiEvent::PointerMotion(motion) => Raw::Motion {
            dx: motion.dx,
            dy: motion.dy,
        },
        EiEvent::KeyboardKey(key) => Raw::Key {
            code: key.key,
            down: key.state == ei::keyboard::KeyState::Press,
        },
        EiEvent::Button(button) => Raw::Button {
            code: button.button,
            down: button.state == ei::button::ButtonState::Press,
        },
        EiEvent::ScrollDelta(scroll) => Raw::ScrollSmooth {
            dx: scroll.dx,
            dy: scroll.dy,
        },
        EiEvent::ScrollDiscrete(scroll) => Raw::ScrollDiscrete {
            dx: scroll.discrete_dx,
            dy: scroll.discrete_dy,
        },
        EiEvent::Frame(_) => Raw::Frame,
        _ => return None,
    };
    Some(raw)
}

fn ei_error(error: &reis::Error) -> InputError {
    InputError::Portal(format!("libei failed: {error}"))
}

fn flush_error(error: rustix::io::Errno) -> InputError {
    InputError::Portal(format!("cannot write to the EI socket: {error}"))
}
