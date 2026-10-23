// SPDX-License-Identifier: GPL-3.0-or-later

//! Inject thread: `XTest` fake input.

use std::ops::ControlFlow;
use std::thread;

use a7n_proto::v1::{ButtonEvent, InputEvent, KeyEvent, ScrollEvent, input_event};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{Receiver, Sender};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
};
use x11rb::protocol::xtest::{self, ConnectionExt as _};
use x11rb::{CURRENT_TIME, NONE};

use super::buttons::{scroll_clicks, x_button_from};
use super::keymap::keycode_from_hid;
use super::{Bounds, Session};
use crate::input::scroll::ScrollAccumulator;
use crate::input::{Captured, Inject, InputError};

/// `detail` of an `XTest` `MotionNotify` that is relative to the pointer.
const MOTION_RELATIVE: u8 = 1;
const XTEST_MAJOR_VERSION: u8 = 2;
const XTEST_MINOR_VERSION: u16 = 2;
/// `XTest` sends to the core pointer when no device is named.
const CORE_DEVICE: u8 = 0;
const THREAD_NAME: &str = "a7n-x11-inject";

pub(super) struct InjectThread {
    session: Session,
    scroll: ScrollAccumulator,
}

impl InjectThread {
    pub(super) fn new(session: Session) -> Result<Self, InputError> {
        session.require_extension(xtest::X11_EXTENSION_NAME)?;
        session
            .conn
            .xtest_get_version(XTEST_MAJOR_VERSION, XTEST_MINOR_VERSION)?
            .reply()?;
        Ok(Self {
            session,
            scroll: ScrollAccumulator::default(),
        })
    }

    pub(super) fn bounds(&self) -> Bounds {
        self.session.bounds
    }

    pub(super) fn spawn(
        self,
        commands: Receiver<Inject>,
        pointer: Sender<Captured>,
    ) -> Result<(), InputError> {
        thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || self.run(commands, &pointer))
            .map(drop)
            .map_err(InputError::Thread)
    }

    fn run(mut self, mut commands: Receiver<Inject>, pointer: &Sender<Captured>) {
        while let Some(command) = commands.blocking_recv() {
            if self.handle(command, pointer).is_break() {
                return;
            }
        }
    }

    /// Breaks when the X connection fails or the daemon stopped listening.
    fn handle(&mut self, command: Inject, pointer: &Sender<Captured>) -> ControlFlow<()> {
        let report = match command {
            Inject::Event(event) => self.event(&event).map(|()| None),
            Inject::Motion { dx, dy } => self.motion(dx, dy).map(|()| Some((dx, dy))),
            Inject::Warp { x, y } => self.warp(x, y).map(|()| Some((0, 0))),
        };
        let Ok(report) = report else {
            return ControlFlow::Break(());
        };
        let Some((dx, dy)) = report else {
            return ControlFlow::Continue(());
        };
        let Ok((x, y)) = self.session.pointer_position() else {
            return ControlFlow::Break(());
        };
        // Never block: a daemon that is not draining position reports must not
        // stall injection. A dropped report is superseded by the next one.
        match pointer.try_send(Captured::Pointer { x, y, dx, dy }) {
            Err(TrySendError::Closed(_)) => ControlFlow::Break(()),
            Ok(()) | Err(TrySendError::Full(_)) => ControlFlow::Continue(()),
        }
    }

    fn event(&mut self, event: &InputEvent) -> Result<(), x11rb::errors::ConnectionError> {
        // Text and gestures have no XTest equivalent: typing characters would
        // need keymap rewriting and gestures are a client-side toolkit concept.
        match &event.event {
            Some(input_event::Event::Key(key)) => self.key(key),
            Some(input_event::Event::Button(button)) => self.button(*button),
            Some(input_event::Event::Scroll(scroll)) => self.scroll(scroll),
            Some(input_event::Event::Gesture(_) | input_event::Event::Text(_)) | None => Ok(()),
        }
    }

    fn key(&self, key: &KeyEvent) -> Result<(), x11rb::errors::ConnectionError> {
        let Some(keycode) = keycode_from_hid(key.usage) else {
            return Ok(());
        };
        self.fake(
            press_type(key.down, KEY_PRESS_EVENT, KEY_RELEASE_EVENT),
            keycode,
        )?;
        self.session.conn.flush()
    }

    fn button(&self, event: ButtonEvent) -> Result<(), x11rb::errors::ConnectionError> {
        let Some(button) = x_button_from(event.button()) else {
            return Ok(());
        };
        self.fake(
            press_type(event.down, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT),
            button,
        )?;
        self.session.conn.flush()
    }

    fn scroll(&mut self, event: &ScrollEvent) -> Result<(), x11rb::errors::ConnectionError> {
        let (dx, dy) = self.scroll.push(event.dx, event.dy, event.unit());
        for button in scroll_clicks(dx, dy) {
            self.fake(BUTTON_PRESS_EVENT, button)?;
            self.fake(BUTTON_RELEASE_EVENT, button)?;
        }
        self.session.conn.flush()
    }

    fn motion(&self, dx: i32, dy: i32) -> Result<(), x11rb::errors::ConnectionError> {
        self.session.conn.xtest_fake_input(
            MOTION_NOTIFY_EVENT,
            MOTION_RELATIVE,
            CURRENT_TIME,
            NONE,
            clamp_i16(dx),
            clamp_i16(dy),
            CORE_DEVICE,
        )?;
        self.session.conn.flush()
    }

    fn warp(&self, x: i32, y: i32) -> Result<(), x11rb::errors::ConnectionError> {
        let root = self.session.root;
        self.session
            .conn
            .warp_pointer(NONE, root, 0, 0, 0, 0, clamp_i16(x), clamp_i16(y))?;
        self.session.conn.flush()
    }

    fn fake(&self, kind: u8, detail: u8) -> Result<(), x11rb::errors::ConnectionError> {
        self.session
            .conn
            .xtest_fake_input(kind, detail, CURRENT_TIME, NONE, 0, 0, CORE_DEVICE)
            .map(drop)
    }
}

fn press_type(down: bool, press: u8, release: u8) -> u8 {
    if down { press } else { release }
}

fn clamp_i16(value: i32) -> i16 {
    i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX })
}
