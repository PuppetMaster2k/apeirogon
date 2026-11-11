// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::future::pending;
use std::time::Instant;

use a7n_core::focus::edge::{EdgeResistance, FullscreenHold, HeldButtons};
use a7n_core::focus::{Action, Cursor, Focus, Take};
use a7n_core::hotkey::{HotkeyMatcher, SHAKE_DEFAULT, ShakeDetector};
use a7n_core::input::{MotionCounter, MotionScale, MotionTracker};
use a7n_platform::feedback::Feedback;
use a7n_platform::input::{Bounds, Capture, CaptureControl, Captured, Inject, Injector};
use a7n_proto::v1::{Edge, Envelope, InputBatch, envelope::Body, input_event::Event};
use a7n_proto::{MachineId, Route};
use tokio::sync::oneshot;

use super::{Daemon, DaemonError};

mod actions;
mod crossing;
pub(in crate::daemon) mod feedback_sink;
mod gestures;
mod hotkeys;
mod recording;

use a7n_core::input::landing_point;
use feedback_sink::FeedbackSink;

pub(super) struct Input {
    pub(super) focus: Focus,
    pub(super) capture: Option<Capture>,
    pub(super) injector: Option<Injector>,
    pub(super) unavailable: Option<String>,
    pub(super) paused: bool,
    trace_keys: bool,
    counter: MotionCounter,
    tracker: MotionTracker,
    pub(super) scale: MotionScale,
    pub(super) modifier_remap: BTreeMap<u32, u32>,
    pub(super) hotkeys: HotkeyMatcher,
    pub(super) gestures: a7n_core::gestures::Bindings,
    pub(super) shake: ShakeDetector,
    pub(super) recording: recording::Recording,
    pub(super) fullscreen_hold: FullscreenHold,
    pub(super) resistance: EdgeResistance,
    pub(super) sink: FeedbackSink,
    edge_no_target: Option<Edge>,
    buttons: HeldButtons,
    pub(super) failed: bool,
    lost: Vec<MachineId>,
    /// Outcome of waiting for input permissions at startup or after revocation.
    pub(super) retried: Option<oneshot::Receiver<Retried>>,
}

pub(crate) type Handles = (Capture, Injector);
/// Started input, or why it still cannot start once nothing is missing.
pub(crate) type Retried = Result<Handles, String>;

impl Input {
    pub(super) fn new(own: MachineId) -> Self {
        Self {
            focus: Focus::new(own),
            capture: None,
            injector: None,
            unavailable: None,
            paused: false,
            trace_keys: false,
            counter: MotionCounter::default(),
            tracker: MotionTracker::default(),
            scale: MotionScale::new(1.0),
            modifier_remap: BTreeMap::new(),
            hotkeys: HotkeyMatcher::new(a7n_ipc::config::Config::default().bindings()),
            gestures: a7n_ipc::config::Config::default().gesture_bindings(),
            shake: ShakeDetector::new(SHAKE_DEFAULT),
            recording: recording::Recording::default(),
            fullscreen_hold: FullscreenHold::new(false),
            resistance: EdgeResistance::new(0),
            sink: FeedbackSink::default(),
            edge_no_target: None,
            buttons: HeldButtons::default(),
            failed: false,
            lost: Vec::new(),
            retried: None,
        }
    }

    fn reset_edge_state(&mut self) {
        self.buttons.reset();
        self.fullscreen_hold.reset();
        self.sink.clear();
        self.edge_no_target = None;
    }

    pub(super) fn poll_edge_pressure(&mut self) {
        self.resistance.poll(now());
        if self.paused || (!self.focus.is_driven() && self.focus.is_locked()) {
            self.sink.clear();
            return;
        }
        self.sink.fade(self.resistance.progress());
    }

    pub(super) fn keepalive(&mut self) {
        if let Some(capture) = &self.capture {
            if matches!(
                capture.control.try_send(CaptureControl::Keepalive),
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_))
            ) {
                self.closed();
            }
        }
    }

    pub(super) fn closed(&mut self) {
        if !self.failed {
            crate::log::event!("kind=input_backend_closed");
        }
        self.failed = true;
        self.unavailable = Some("input backend closed".to_owned());
    }

    pub(super) async fn grab(&mut self, grabbed: bool) {
        if let Some(capture) = &self.capture {
            if capture
                .control
                .send(CaptureControl::Grab(grabbed))
                .await
                .is_err()
            {
                self.closed();
            }
        }
    }

    fn trace_key(&self, event: &a7n_proto::v1::InputEvent) {
        if !self.trace_keys {
            return;
        }
        if let Some(a7n_proto::v1::input_event::Event::Key(key)) = &event.event {
            crate::log::event!("kind=trace_key usage=0x{:x} down={}", key.usage, key.down);
        }
    }

    async fn inject(&mut self, command: Inject) {
        if let Inject::Event(event) = &command {
            self.trace_key(event);
            if let Some(Event::Button(button)) = &event.event {
                self.buttons.apply(button);
            }
        }
        if let Some(injector) = &self.injector {
            if injector.commands.send(command).await.is_err() {
                self.closed();
            }
        }
    }
}

pub(super) async fn captured(capture: &mut Option<Capture>) -> Option<Captured> {
    match capture {
        Some(capture) => capture.events.recv().await,
        None => pending().await,
    }
}

/// Resolves once, when the retry task finishes or is dropped.
pub(super) async fn retried(retried: &mut Option<oneshot::Receiver<Retried>>) -> Option<Retried> {
    match retried {
        Some(receiver) => receiver.await.ok(),
        None => pending().await,
    }
}

pub(super) async fn feedback(injector: &mut Option<Injector>) -> Option<Captured> {
    match injector {
        Some(injector) => injector.pointer.recv().await,
        None => pending().await,
    }
}

pub(super) async fn deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => pending().await,
    }
}

impl Daemon {
    pub(crate) fn with_input_error(mut self, error: Option<String>) -> Self {
        self.input.unavailable = error;
        self
    }

    pub(crate) fn with_trace_input(mut self, enabled: bool) -> Self {
        self.input.trace_keys = enabled;
        self
    }

    pub(crate) fn with_feedback(mut self, feedback: Option<Feedback>) -> Self {
        self.input.sink.attach(feedback);
        self
    }

    pub(crate) fn with_input(mut self, handles: Option<Handles>) -> Self {
        if let Some(handles) = handles {
            self.attach_input(handles);
        }
        self
    }

    /// Input that could not start yet arrives here once its permissions are granted.
    pub(crate) fn with_input_retry(mut self, retried: oneshot::Receiver<Retried>) -> Self {
        self.input.retried = Some(retried);
        self
    }

    /// Takes the retry outcome; `None` means the task ended without one.
    pub(super) fn finish_input_retry(&mut self, retried: Option<Retried>) {
        self.input.retried = None;
        match retried {
            Some(Ok(handles)) => self.attach_input(handles),
            // The old reason named a permission that is granted by now.
            Some(Err(reason)) => self.input.unavailable = Some(reason),
            None => {}
        }
    }

    fn attach_input(&mut self, (capture, injector): Handles) {
        self.input.capture = Some(capture);
        self.input.injector = Some(injector);
        self.input.unavailable = None;
        self.input.failed = false;
    }

    pub(super) async fn retry_input_after_permission_loss(&mut self) {
        crate::log::event!("kind=input_permission_lost");
        let actions = self.input.focus.go_home();
        self.input_actions(actions).await;
        let peers: Vec<_> = self.links.by_peer.keys().copied().collect();
        for peer in peers {
            let actions = self.input.focus.deny_drive(peer);
            self.input_actions(actions).await;
        }
        self.input.grab(false).await;
        self.input.capture = None;
        self.input.injector = None;
        self.input.failed = false;
        self.input.unavailable = Some("input permissions missing".to_owned());
        let (handles, retried) = oneshot::channel();
        self.input.retried = Some(retried);
        crate::input_retry::spawn(handles);
    }

    pub(super) async fn shutdown_input(&mut self) {
        let peers: Vec<_> = self.links.by_peer.keys().copied().collect();
        for peer in peers {
            self.input_link_lost(peer);
        }
        self.flush_link_losses().await;
        self.input.grab(false).await;
    }

    pub(super) async fn poll_focus(&mut self) {
        let actions = self.input.focus.poll(now());
        self.input_actions(actions).await;
    }

    pub(super) fn input_link_lost(&mut self, peer: MachineId) {
        self.sync.drop_peer(peer);
        self.clipboard_link_down(peer);
        self.files_link_down(peer);
        self.input.lost.push(peer);
    }

    pub(super) async fn flush_link_losses(&mut self) {
        while let Some(peer) = self.input.lost.pop() {
            let actions = self.input.focus.on_link_lost(peer);
            self.input_actions(actions).await;
            self.sync_grab().await;
        }
    }

    pub(super) fn send_input(&mut self, peer: MachineId, body: Body) {
        let Some(handle) = self.links.by_peer.get(&peer) else {
            return;
        };
        if handle
            .sender
            .try_send(Envelope { body: Some(body) })
            .is_err()
        {
            handle.connection.close(crate::link::CLOSE_NORMAL, b"");
            self.links.by_peer.remove(&peer);
            self.input_link_lost(peer);
        }
    }

    pub(super) async fn receive_from(
        &mut self,
        peer: MachineId,
        envelope: Envelope,
    ) -> Result<(), DaemonError> {
        self.poll_focus().await;
        let actions = match &envelope.body {
            Some(Body::TransferRequest(request)) => {
                if !self.files_request(peer, request) {
                    self.clipboard_request(peer, request);
                }
                return Ok(());
            }
            Some(Body::ClipboardAnnounce(wire)) => {
                if wire.send {
                    self.receive_file_offer(peer, wire);
                } else {
                    self.receive_clipboard(peer, wire);
                }
                return Ok(());
            }
            Some(Body::TransferCancel(cancel)) => {
                self.files_cancel_from(peer, cancel.transfer_id);
                return Ok(());
            }
            Some(Body::Layout(_) | Body::Settings(_) | Body::Desk(_)) => {
                return self.receive_sync_from(peer, envelope, Instant::now()).await;
            }
            Some(Body::Focus(focus)) => {
                // Without an injector there is no backend that can honour a take.
                let allow_drive = self.allow_drive(peer);
                self.input.focus.on_focus(peer, focus, allow_drive, now())
            }
            Some(Body::Accept(accept)) => self.input.focus.on_accept(peer, accept),
            Some(Body::Accepts(accepts)) => self.input.focus.on_accepts(peer, accepts.accepts),
            Some(Body::Refuse(refuse)) => self.input.focus.on_refuse(peer, refuse),
            Some(Body::Handover(handover)) if !self.input.paused => {
                self.input.focus.on_handover(peer, handover, now())
            }
            Some(Body::Ack(ack)) => self.input.focus.on_ack(peer, ack),
            Some(Body::Motion(motion)) => {
                if self
                    .input
                    .focus
                    .on_remote_input(peer, Route(motion.route), now())
                {
                    let delta = self
                        .input
                        .tracker
                        .apply(motion.motion_seq, motion.x, motion.y);
                    self.inject_motion(delta).await;
                }
                return Ok(());
            }
            Some(Body::Input(batch)) => {
                self.receive_input(peer, batch).await;
                return Ok(());
            }
            _ => return self.receive(envelope),
        };
        self.input_actions(actions).await;
        self.sync_grab().await;
        Ok(())
    }

    async fn receive_input(&mut self, peer: MachineId, batch: &InputBatch) {
        let route = Route(batch.route);
        if !self.input.focus.on_remote_input(peer, route, now()) {
            return;
        }
        let delta = self
            .input
            .tracker
            .resync(batch.motion_seq, batch.motion_x, batch.motion_y);
        self.inject_motion(delta).await;
        let mut gesture_seen = false;
        for event in &batch.events {
            if !self.input.focus.on_input_event(peer, route, event, now()) {
                continue;
            }
            if let Some(Event::Gesture(gesture)) = &event.event {
                if !std::mem::replace(&mut gesture_seen, true) {
                    self.inject_gesture(peer, *gesture).await;
                }
                continue;
            }
            self.input.inject(Inject::Event(event.clone())).await;
        }
    }

    async fn inject_motion(&mut self, delta: Option<(i32, i32)>) {
        if let Some((dx, dy)) = delta {
            let (dx, dy) = self.input.scale.apply(dx, dy);
            self.input.inject(Inject::Motion { dx, dy }).await;
        }
    }

    pub(super) async fn captured(&mut self, event: Option<Captured>) {
        let Some(event) = event else {
            self.input.closed();
            return;
        };
        if self.record_capture(&event) {
            return;
        }
        if let Captured::Input(input) = &event {
            self.input.trace_key(input);
        }
        self.poll_focus().await;
        if self.match_hotkey(&event).await {
            return;
        }
        if let Cursor::Driving(peer) = self.input.focus.cursor() {
            if self.match_shake(&event).await {
                return;
            }
            self.log_driving_gesture(peer, &event);
            self.forward_capture(peer, event);
            return;
        }
        let was_driven = self.input.focus.is_driven();
        let (allowed, actions) = self.input.focus.on_local_input(now());
        self.input_actions(actions).await;
        if !allowed {
            return;
        }
        if let Captured::Input(input) = &event {
            if let Some(Event::Button(button)) = &input.event {
                self.input.buttons.apply(button);
            }
        }
        if was_driven {
            self.sync_grab().await;
            match event {
                Captured::Input(event) => self.input.inject(Inject::Event(event)).await,
                Captured::Pointer { dx, dy, .. } => {
                    self.input.inject(Inject::Motion { dx, dy }).await;
                }
            }
            return;
        }
        if let (Captured::Pointer { x, y, dx, dy }, Some(capture)) = (event, &self.input.capture) {
            let (bounds, monitors) = (capture.bounds, capture.monitors.clone());
            self.cross_edge(bounds, &monitors, (x, y), (dx, dy), false)
                .await;
        }
    }

    fn forward_capture(&mut self, peer: MachineId, event: Captured) {
        let Some(route) = self.input.focus.driving_route() else {
            return;
        };
        let body = match event {
            Captured::Pointer { dx, dy, .. } => {
                let mut motion = self.input.counter.push(dx, dy);
                motion.route = route.0;
                Body::Motion(motion)
            }
            Captured::Input(mut event) => {
                if !self.gesture_allowed(peer, &event) {
                    return;
                }
                self.transform_input(&mut event);
                let motion = self.input.counter.current();
                // ponytail: one event per batch; coalesce when capture throughput needs it.
                Body::Input(InputBatch {
                    route: route.0,
                    seq: 0,
                    motion_seq: motion.motion_seq,
                    motion_x: motion.x,
                    motion_y: motion.y,
                    events: vec![event],
                })
            }
        };
        self.send_input(peer, body);
    }

    pub(super) async fn pointer_feedback(&mut self, event: Option<Captured>) {
        let Some(event) = event else {
            self.input.closed();
            return;
        };
        if !self.input.focus.is_driven() {
            return;
        }
        if let (Captured::Pointer { x, y, dx, dy }, Some(injector)) = (event, &self.input.injector)
        {
            let (bounds, monitors) = (injector.bounds, injector.monitors.clone());
            self.cross_edge(bounds, &monitors, (x, y), (dx, dy), true)
                .await;
        }
    }
}

// Tokio time also supports deterministic paused-time tests of the core's deadlines.
fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

#[cfg(test)]
mod tests;
