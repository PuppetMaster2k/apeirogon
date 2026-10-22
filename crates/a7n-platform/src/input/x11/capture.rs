// SPDX-License-Identifier: GPL-3.0-or-later

//! Capture thread: `XInput2` raw events plus keyboard/pointer grabs.

use std::io;
use std::ops::ControlFlow;
use std::thread;
use std::time::{Duration, Instant};

use a7n_proto::v1::{ButtonEvent, InputEvent, KeyEvent, ScrollEvent, ScrollUnit, input_event};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::mpsc::{Receiver, Sender};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{self, ConnectionExt as _};
use x11rb::protocol::xinput::{
    self, ConnectionExt as _, DeviceType, EventMask, KeyEventFlags, XIEventMask,
};
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, GrabStatus};
use x11rb::rust_connection::RustConnection;
use x11rb::{CURRENT_TIME, NONE};

use super::accumulate::MotionAccumulator;
use super::buttons::{PointerButton, classify_x_button};
use super::keymap::hid_from_keycode;
use super::{Bounds, Session};
use crate::input::{CaptureControl, Captured, DeadMan, InputError};

/// Longest wait on the X connection before control commands and the dead-man
/// switch are looked at again.
const WAKE_INTERVAL: Duration = Duration::from_millis(50);
/// `XInput2` pseudo device id selecting every master device.
const ALL_MASTER_DEVICES: u16 = 1;
const XI_MAJOR_VERSION: u16 = 2;
const XI_MINOR_VERSION: u16 = 0;
const XFIXES_MAJOR_VERSION: u32 = 4;
const XFIXES_MINOR_VERSION: u32 = 0;
/// Valuator numbers of the pointer's x and y axes.
const AXIS_X: u32 = 0;
const AXIS_Y: u32 = 1;
/// `XInput2` pseudo device id addressing every device, used for hierarchy events.
const ALL_DEVICES: u16 = 0;
/// Xorg names its XTEST slave devices "Virtual core XTEST pointer/keyboard".
const XTEST_DEVICE_NAME: &str = "XTEST";
const THREAD_NAME: &str = "a7n-x11-capture";

pub(super) struct CaptureThread {
    session: Session,
    motion: MotionAccumulator,
    deadman: DeadMan,
    grabbed: bool,
    cursor_hidden: bool,
    /// Slave devices fed by XTEST, i.e. our own injection.
    xtest_sources: Vec<u16>,
}

impl CaptureThread {
    pub(super) fn new(session: Session) -> Result<Self, InputError> {
        session.require_extension(xinput::X11_EXTENSION_NAME)?;
        session.require_extension(xfixes::X11_EXTENSION_NAME)?;
        let version = session
            .conn
            .xinput_xi_query_version(XI_MAJOR_VERSION, XI_MINOR_VERSION)?
            .reply()?;
        if version.major_version < XI_MAJOR_VERSION {
            return Err(InputError::MissingX11Extension("XInput 2.0"));
        }
        session
            .conn
            .xfixes_query_version(XFIXES_MAJOR_VERSION, XFIXES_MINOR_VERSION)?
            .reply()?;
        let hierarchy = EventMask {
            deviceid: ALL_DEVICES,
            mask: vec![XIEventMask::HIERARCHY],
        };
        let mask = EventMask {
            deviceid: ALL_MASTER_DEVICES,
            mask: vec![
                XIEventMask::RAW_MOTION
                    | XIEventMask::RAW_KEY_PRESS
                    | XIEventMask::RAW_KEY_RELEASE
                    | XIEventMask::RAW_BUTTON_PRESS
                    | XIEventMask::RAW_BUTTON_RELEASE,
            ],
        };
        session
            .conn
            .xinput_xi_select_events(session.root, &[mask, hierarchy])?
            .check()?;
        let xtest_sources = query_xtest_sources(&session.conn)?;
        Ok(Self {
            session,
            xtest_sources,
            motion: MotionAccumulator::default(),
            deadman: DeadMan::new(Instant::now()),
            grabbed: false,
            cursor_hidden: false,
        })
    }

    pub(super) fn bounds(&self) -> Bounds {
        self.session.bounds
    }

    pub(super) fn spawn(
        self,
        events: Sender<Captured>,
        control: Receiver<CaptureControl>,
    ) -> Result<(), InputError> {
        thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || self.run(&events, control))
            .map(drop)
            .map_err(InputError::Thread)
    }

    fn run(mut self, events: &Sender<Captured>, mut control: Receiver<CaptureControl>) {
        while self.step(events, &mut control).is_continue() {}
        // Whatever ended the loop, the local user gets the devices back.
        self.release();
    }

    fn step(
        &mut self,
        events: &Sender<Captured>,
        control: &mut Receiver<CaptureControl>,
    ) -> ControlFlow<()> {
        self.drain_events(events)?;
        self.drain_control(control)?;
        if self.grabbed && self.deadman.expired(Instant::now()) {
            self.release();
        }
        self.wait_readable()
    }

    fn drain_events(&mut self, events: &Sender<Captured>) -> ControlFlow<()> {
        loop {
            let event = match self.session.conn.poll_for_event() {
                Ok(Some(event)) => event,
                Ok(None) => return ControlFlow::Continue(()),
                Err(_) => return ControlFlow::Break(()),
            };
            let Ok(captured) = self.translate(&event) else {
                return ControlFlow::Break(());
            };
            if let Some(captured) = captured {
                // Never block here: a stalled daemon must not keep input grabbed,
                // so a full queue drops the event and the dead-man switch
                // still gets its turn. A closed queue means the daemon is gone.
                if let Err(TrySendError::Closed(_)) = events.try_send(captured) {
                    return ControlFlow::Break(());
                }
            }
        }
    }

    fn drain_control(&mut self, control: &mut Receiver<CaptureControl>) -> ControlFlow<()> {
        loop {
            match control.try_recv() {
                Ok(command) => self.apply(command),
                Err(TryRecvError::Empty) => return ControlFlow::Continue(()),
                Err(TryRecvError::Disconnected) => return ControlFlow::Break(()),
            }
        }
    }

    fn apply(&mut self, command: CaptureControl) {
        let now = Instant::now();
        match command {
            CaptureControl::Keepalive => self.deadman.keepalive(now),
            CaptureControl::Grab(true) => {
                // A grab request is itself proof that the daemon is alive.
                self.deadman.keepalive(now);
                self.grab();
            }
            CaptureControl::Grab(false) => self.release(),
        }
    }

    fn wait_readable(&self) -> ControlFlow<()> {
        let result = wait_for_input(&self.session.conn);
        match result {
            Ok(()) => ControlFlow::Continue(()),
            Err(_) => ControlFlow::Break(()),
        }
    }

    /// Raw events are not affected by grabs, so capture keeps working while
    /// local applications receive nothing.
    fn grab(&mut self) {
        if self.grabbed {
            return;
        }
        if self.try_grab().is_some() {
            self.grabbed = true;
            return;
        }
        // Never leave the local user half locked out.
        self.release();
        eprintln!("kind=input_grab_failed");
    }

    fn try_grab(&mut self) -> Option<()> {
        let conn = &self.session.conn;
        let root = self.session.root;
        let keyboard = conn
            .grab_keyboard(false, root, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)
            .ok()?
            .reply()
            .ok()?;
        (keyboard.status == GrabStatus::SUCCESS).then_some(())?;
        let pointer = conn
            .grab_pointer(
                false,
                root,
                x11rb::protocol::xproto::EventMask::NO_EVENT,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                NONE,
                NONE,
                CURRENT_TIME,
            )
            .ok()?
            .reply()
            .ok()?;
        (pointer.status == GrabStatus::SUCCESS).then_some(())?;
        conn.xfixes_hide_cursor(root).ok()?;
        self.cursor_hidden = true;
        conn.flush().ok()
    }

    /// Best effort: if the connection is already gone, the server releases
    /// grabs and the cursor by itself.
    fn release(&mut self) {
        let conn = &self.session.conn;
        let _ = conn.ungrab_keyboard(CURRENT_TIME);
        let _ = conn.ungrab_pointer(CURRENT_TIME);
        if self.cursor_hidden {
            let _ = conn.xfixes_show_cursor(self.session.root);
        }
        let _ = conn.flush();
        self.grabbed = false;
        self.cursor_hidden = false;
    }

    fn translate(&mut self, event: &Event) -> Result<Option<Captured>, x11rb::errors::ReplyError> {
        if let Event::XinputHierarchy(_) = event {
            self.xtest_sources = query_xtest_sources(&self.session.conn)?;
            return Ok(None);
        }
        if is_injected(&self.xtest_sources, source_id(event)) {
            return Ok(None);
        }
        match event {
            Event::XinputRawMotion(raw) => {
                let (dx, dy) = raw_axes(&raw.valuator_mask, &raw.axisvalues_raw);
                let (dx, dy) = self.motion.push(dx, dy);
                if dx == 0 && dy == 0 {
                    return Ok(None);
                }
                let (x, y) = self.session.pointer_position()?;
                Ok(Some(Captured::Pointer { x, y, dx, dy }))
            }
            Event::XinputRawKeyPress(raw) => {
                let repeat = raw.flags.contains(KeyEventFlags::KEY_REPEAT);
                Ok((!repeat).then(|| key(raw.detail, true)).flatten())
            }
            Event::XinputRawKeyRelease(raw) => Ok(key(raw.detail, false)),
            Event::XinputRawButtonPress(raw) => Ok(button(raw.detail, true)),
            Event::XinputRawButtonRelease(raw) => Ok(button(raw.detail, false)),
            _ => Ok(None),
        }
    }
}

/// Our own XTEST injection comes back as raw events; reporting it as local
/// activity would make a driven machine look like it is being used locally.
fn is_injected(xtest_sources: &[u16], source: Option<u16>) -> bool {
    source.is_some_and(|source| xtest_sources.contains(&source))
}

fn source_id(event: &Event) -> Option<u16> {
    match event {
        Event::XinputRawMotion(raw)
        | Event::XinputRawButtonPress(raw)
        | Event::XinputRawButtonRelease(raw) => Some(raw.sourceid),
        Event::XinputRawKeyPress(raw) | Event::XinputRawKeyRelease(raw) => Some(raw.sourceid),
        _ => None,
    }
}

fn query_xtest_sources(conn: &RustConnection) -> Result<Vec<u16>, x11rb::errors::ReplyError> {
    let devices = conn.xinput_xi_query_device(ALL_DEVICES)?.reply()?;
    Ok(devices
        .infos
        .iter()
        .filter(|info| {
            info.type_ == DeviceType::SLAVE_POINTER || info.type_ == DeviceType::SLAVE_KEYBOARD
        })
        .filter(|info| String::from_utf8_lossy(&info.name).contains(XTEST_DEVICE_NAME))
        .map(|info| info.deviceid)
        .collect())
}

fn wait_for_input(conn: &RustConnection) -> io::Result<()> {
    let timeout = Timespec::try_from(WAKE_INTERVAL).map_err(io::Error::other)?;
    let mut fds = [PollFd::new(conn.stream(), PollFlags::IN)];
    match poll(&mut fds, Some(&timeout)) {
        Ok(_) | Err(Errno::INTR) => Ok(()),
        Err(errno) => Err(errno.into()),
    }
}

/// Unaccelerated x and y of a raw event. `values` holds one entry per set bit
/// of `mask`, in valuator order.
fn raw_axes(mask: &[u32], values: &[xinput::Fp3232]) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    let set_bits = mask.iter().enumerate().flat_map(|(word, bits)| {
        (0..u32::BITS)
            .filter(move |bit| bits & (1 << bit) != 0)
            .map(move |bit| word_axis(word, bit))
    });
    for (axis, value) in set_bits.zip(values) {
        let value = f64::from(value.integral) + f64::from(value.frac) / FRAC_SCALE;
        match axis {
            AXIS_X => x = value,
            AXIS_Y => y = value,
            _ => {}
        }
    }
    (x, y)
}

/// A `Fp3232` fraction is in units of 2^-32.
const FRAC_SCALE: f64 = 4_294_967_296.0;

fn word_axis(word: usize, bit: u32) -> u32 {
    u32::try_from(word)
        .unwrap_or(u32::MAX)
        .saturating_mul(u32::BITS)
        + bit
}

fn key(keycode: u32, down: bool) -> Option<Captured> {
    let usage = hid_from_keycode(keycode)?;
    Some(Captured::Input(InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage,
            down,
            character: String::new(),
        })),
    }))
}

fn button(detail: u32, down: bool) -> Option<Captured> {
    let event = match classify_x_button(detail)? {
        PointerButton::Click(button) => input_event::Event::Button(ButtonEvent {
            button: button.into(),
            down,
        }),
        // A notch is one press; the release carries nothing.
        PointerButton::Scroll { .. } if !down => return None,
        PointerButton::Scroll { dx, dy } => input_event::Event::Scroll(ScrollEvent {
            dx: f32::from(dx),
            dy: f32::from(dy),
            unit: ScrollUnit::Line.into(),
        }),
    };
    Some(Captured::Input(InputEvent { event: Some(event) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_injected_xtest_source_dropped_real_source_kept() {
        // Arrange
        let xtest_sources = [4, 5];

        // Act
        let verdicts = [Some(4), Some(9), None].map(|source| is_injected(&xtest_sources, source));

        // Assert
        assert_eq!(verdicts, [true, false, false]);
    }
}
