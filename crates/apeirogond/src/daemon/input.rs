// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::future::pending;
use std::time::Instant;

use a7n_core::focus::edge::EdgeResistance;
use a7n_core::focus::{Action, Cursor, Focus, Take};
use a7n_core::hotkey::HotkeyMatcher;
use a7n_core::input::{MotionCounter, MotionScale, MotionTracker, edge_push};
use a7n_platform::input::{Bounds, Capture, CaptureControl, Captured, Inject, Injector};
use a7n_proto::v1::{Edge, Envelope, InputBatch, envelope::Body};
use a7n_proto::{MachineId, Route};

use super::{Daemon, DaemonError};

mod hotkeys;
mod landing;

use landing::landing_point;

pub(super) struct Input {
    pub(super) focus: Focus,
    pub(super) capture: Option<Capture>,
    pub(super) injector: Option<Injector>,
    counter: MotionCounter,
    tracker: MotionTracker,
    pub(super) scale: MotionScale,
    pub(super) modifier_remap: BTreeMap<u32, u32>,
    pub(super) hotkeys: HotkeyMatcher,
    resistance: EdgeResistance,
    pub(super) failed: bool,
    lost: Vec<MachineId>,
}

impl Input {
    pub(super) fn new(own: MachineId) -> Self {
        Self {
            focus: Focus::new(own),
            capture: None,
            injector: None,
            counter: MotionCounter::default(),
            tracker: MotionTracker::default(),
            scale: MotionScale::new(1.0),
            modifier_remap: BTreeMap::new(),
            hotkeys: HotkeyMatcher::new(a7n_ipc::config::Config::default().bindings()),
            resistance: EdgeResistance::new(0),
            failed: false,
            lost: Vec::new(),
        }
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
            eprintln!("kind=input_backend_closed");
        }
        self.failed = true;
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

    async fn inject(&mut self, command: Inject) {
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
    pub(crate) fn with_input(mut self, handles: Option<(Capture, Injector)>) -> Self {
        if let Some((capture, injector)) = handles {
            self.input.capture = Some(capture);
            self.input.injector = Some(injector);
        }
        self
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
        self.input.lost.push(peer);
    }

    pub(super) async fn flush_link_losses(&mut self) {
        while let Some(peer) = self.input.lost.pop() {
            let actions = self.input.focus.on_link_lost(peer);
            self.input_actions(actions).await;
            self.sync_grab().await;
        }
    }

    pub(super) async fn sync_grab(&mut self) {
        self.input
            .grab(
                self.input.focus.is_driven()
                    || matches!(self.input.focus.cursor(), Cursor::Driving(_)),
            )
            .await;
    }

    pub(super) async fn input_actions(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send { peer, body } => self.send_input(peer, body),
                Action::Release(events) => {
                    for event in events {
                        self.input.inject(Inject::Event(event)).await;
                    }
                }
                Action::Cursor(cursor) => {
                    if let Cursor::Driving(peer) = cursor {
                        self.input.counter.reset();
                        self.snapshot_remap(peer);
                    }
                    self.input.resistance = EdgeResistance::new(0);
                    self.sync_grab().await;
                }
                Action::Driven {
                    landing,
                    resistance_px,
                    ..
                } => {
                    self.input.tracker.reset();
                    self.input.scale = MotionScale::new(self.config.input.speed);
                    self.input.resistance = EdgeResistance::new(resistance_px);
                    self.sync_grab().await;
                    if let (Some(landing), Some(injector)) = (landing, &self.input.injector) {
                        let (x, y) = landing_point(injector.bounds, &landing);
                        self.input.inject(Inject::Warp { x, y }).await;
                    }
                }
            }
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
            handle.connection.close(0u32.into(), b"");
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
            Some(Body::Settings(settings)) => {
                if self.links.by_peer.get(&peer).is_some_and(|handle| {
                    handle
                        .capabilities
                        .contains(&a7n_proto::v1::Capability::SettingsSync)
                }) {
                    self.receive_settings(settings).await?;
                }
                return Ok(());
            }
            Some(Body::Focus(focus)) => {
                // Without an injector there is no backend that can honour a take.
                let allow_drive = self.allow_drive(peer);
                self.input.focus.on_focus(peer, focus, allow_drive, now())
            }
            Some(Body::Accept(accept)) => self.input.focus.on_accept(peer, accept),
            Some(Body::Accepts(accepts)) => self.input.focus.on_accepts(peer, accepts.accepts),
            Some(Body::Refuse(refuse)) => self.input.focus.on_refuse(peer, refuse),
            Some(Body::Handover(handover)) => self.input.focus.on_handover(peer, handover, now()),
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
        for event in &batch.events {
            if self.input.focus.on_input_event(peer, route, event, now()) {
                self.input.inject(Inject::Event(event.clone())).await;
            }
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
        self.poll_focus().await;
        if self.match_hotkey(&event).await {
            return;
        }
        if let Cursor::Driving(peer) = self.input.focus.cursor() {
            self.forward_capture(peer, event);
            return;
        }
        let was_driven = self.input.focus.is_driven();
        let (allowed, actions) = self.input.focus.on_local_input(now());
        self.input_actions(actions).await;
        if !allowed {
            return;
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
            self.cross_edge(capture.bounds, x, y, dx, dy, false).await;
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
            self.cross_edge(injector.bounds, x, y, dx, dy, true).await;
        }
    }

    async fn cross_edge(&mut self, bounds: Bounds, x: i32, y: i32, dx: i32, dy: i32, driven: bool) {
        if !driven && self.input.focus.is_locked() {
            return;
        }
        let Some((edge, overshoot, along)) = edge_push(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            x,
            y,
            dx,
            dy,
        ) else {
            self.input.resistance.push(Edge::Unspecified, 0);
            return;
        };
        if !self.input.resistance.push(edge, overshoot) {
            return;
        }
        let Some((target, landing)) = self
            .layout
            .state
            .exit(self.identity.machine_id, edge, along)
        else {
            return;
        };
        let actions = if driven {
            self.input.focus.handover(target, Some(landing))
        } else {
            if !self.links.by_peer.contains_key(&target) {
                return;
            }
            self.input.focus.take(
                Take {
                    target,
                    landing: Some(landing),
                    resistance_px: 0,
                    reach: self.links.by_peer.keys().copied().collect(),
                },
                now(),
            )
        };
        self.input_actions(actions).await;
    }
}

// Tokio time also supports deterministic paused-time tests of the core's deadlines.
fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

#[cfg(test)]
mod tests;
