// SPDX-License-Identifier: GPL-3.0-or-later

//! Edge glow, landing ring and haptic ticks on macOS; contract in [`super`].
//!
//! Two click-through, non-activating panels live on the process main thread,
//! which [`crate::main_thread::run`] keeps in its `CFRunLoop` (`AppKit` only
//! allows windows there). A bridge thread blocks on the tokio receiver, passes
//! commands over a std channel and signals a run-loop source on the main loop,
//! so nothing polls. The panels are created lazily inside that source's
//! callback, on the main thread. Without `main_thread::run` there is no main
//! run loop and [`start`] returns `None`, so tests never touch `AppKit`.

mod blit;

use std::ffi::c_void;
use std::sync::mpsc as sync;
use std::thread;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::rc::{Retained, autoreleasepool};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSHapticFeedbackManager, NSHapticFeedbackPattern,
    NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer, NSImageView, NSPanel, NSScreen,
    NSStatusWindowLevel, NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask,
    NSWorkspace,
};
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext,
    kCFRunLoopDefaultMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tokio::sync::mpsc;

use super::look::{self, BEAM_THICKNESS};
use super::macos_geometry::{self as geometry, LANDING_DIAMETER, Rect};
use super::raster::{self, Along, EdgeEffect, Frame};
use super::{Command, Feedback, Look, Style};
use crate::main_thread::{self, Waker};
use blit::Bitmap;

const LANDING_RING_WIDTH: f64 = 4.0;
/// Long enough to notice, short enough not to linger over the new window.
const LANDING_FADE: Duration = Duration::from_millis(700);
/// With Reduce Motion on the ring stays put instead of fading.
const LANDING_STATIC_HOLD: Duration = Duration::from_millis(500);
/// Never fires on its own; every landing re-arms it.
const TIMER_DISARMED_SECONDS: f64 = 1.0e12;
const LANDING_FRAME_SECONDS: f64 = 0.016;

/// State of the main-thread callbacks. Leaked on purpose: it lives as long as
/// the process and is only touched from main run-loop callbacks.
struct Ui {
    commands: sync::Receiver<Command>,
    panels: Option<Panels>,
}

struct Panels {
    mtm: MainThreadMarker,
    glow: Retained<NSPanel>,
    glow_fill: Retained<NSImageView>,
    glow_frame: Frame,
    edge_effect: Option<EdgeEffect>,
    edge_started: Instant,
    landing_visible: bool,
    timer_running: bool,
    glow_bitmap: Bitmap,
    landing_frame: Frame,
    landing_bitmap: Bitmap,
    landing: Retained<NSPanel>,
    landing_fill: Retained<NSImageView>,
    landing_started: Instant,
    landing_center: (f64, f64),
    landing_look: Look,
    animate: bool,
    hide_landing: CFRetained<CFRunLoopTimer>,
}

pub(super) fn start() -> Option<Feedback> {
    if !main_thread::active() {
        return None;
    }
    let (feedback, receiver) = super::channel();
    let (command_tx, command_rx) = sync::channel();
    let ui = Box::leak(Box::new(Ui {
        commands: command_rx,
        panels: None,
    }));
    // SAFETY: `ui` is leaked, so the pointer is valid forever, and
    // `drain_commands` only runs on the main thread, one callback at a time.
    let waker = unsafe {
        Waker::new(
            CFRunLoop::main()?,
            drain_commands,
            std::ptr::from_mut(ui).cast(),
        )
    }?;
    thread::Builder::new()
        .name("a7n-macos-feedback".into())
        .spawn(move || forward_commands(receiver, command_tx, &waker))
        .ok()?;
    Some(feedback)
}

fn forward_commands(
    mut receiver: mpsc::Receiver<Command>,
    commands: sync::Sender<Command>,
    waker: &Waker,
) {
    while let Some(command) = receiver.blocking_recv() {
        if commands.send(command).is_err() {
            return;
        }
        waker.wake();
    }
    // Waking with the sender gone lets the main thread hide the panels.
    drop(commands);
    waker.wake();
}

unsafe extern "C-unwind" fn drain_commands(info: *mut c_void) {
    autoreleasepool(|_| {
        // SAFETY: start passes a leaked, process-long Ui; main run-loop
        // callbacks run one at a time on the main thread.
        let ui = unsafe { &mut *info.cast::<Ui>() };
        loop {
            match ui.commands.try_recv() {
                Ok(command) => ui.apply(command),
                Err(sync::TryRecvError::Empty) => return,
                Err(sync::TryRecvError::Disconnected) => {
                    ui.hide_all();
                    return;
                }
            }
        }
    });
}

unsafe extern "C-unwind" fn hide_landing(_: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: Same contract as drain_commands.
    let ui = unsafe { &mut *info.cast::<Ui>() };
    if let Some(panels) = &mut ui.panels {
        panels.animate_frame();
    }
}

impl Ui {
    fn apply(&mut self, command: Command) {
        match command {
            Command::Tick => tick_haptics(),
            Command::Clear => self.hide_glow(),
            Command::Edge {
                display,
                edge,
                progress,
                look,
            } => {
                if let Some(panels) = self.panels() {
                    panels.show_glow(display, edge, progress, look);
                }
            }
            Command::Landing { x, y, look } => {
                if let Some(panels) = self.panels() {
                    panels.show_landing(x, y, look);
                }
            }
            Command::LandingAtCursor { look } => {
                if let Some(panels) = self.panels() {
                    let point = NSEvent::mouseLocation();
                    panels.show_landing_at((point.x, point.y), look);
                }
            }
        }
    }

    fn hide_glow(&mut self) {
        if let Some(panels) = &mut self.panels {
            panels.edge_effect = None;
            panels.glow.orderOut(None);
            panels.arm_timer();
        }
    }

    fn hide_all(&mut self) {
        self.hide_glow();
        if let Some(panels) = &self.panels {
            panels.landing.orderOut(None);
            panels.hide_landing.invalidate();
        }
    }

    /// The panels, created on first use; callbacks run on the main thread.
    fn panels(&mut self) -> Option<&mut Panels> {
        if self.panels.is_none() {
            let info = std::ptr::from_mut(self).cast();
            self.panels = Panels::new(MainThreadMarker::new()?, info);
        }
        self.panels.as_mut()
    }
}

impl Panels {
    fn new(mtm: MainThreadMarker, timer_info: *mut c_void) -> Option<Self> {
        let glow_fill = image_view(mtm);
        let glow = make_panel(mtm, &glow_fill);
        let landing_fill = image_view(mtm);
        let landing = make_panel(mtm, &landing_fill);
        let mut timer_context = CFRunLoopTimerContext {
            version: 0,
            info: timer_info,
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: The default allocator accepts None; `timer_info` is the
        // leaked Ui that `hide_landing` expects and the context is copied.
        let hide_landing = unsafe {
            CFRunLoopTimer::new(
                None,
                CFAbsoluteTimeGetCurrent() + TIMER_DISARMED_SECONDS,
                LANDING_FRAME_SECONDS,
                0,
                0,
                Some(hide_landing),
                &raw mut timer_context,
            )
        }?;
        // SAFETY: Core Foundation's default run-loop mode is an immutable global.
        CFRunLoop::current()?.add_timer(Some(&hide_landing), unsafe { kCFRunLoopDefaultMode });
        Some(Self {
            mtm,
            glow,
            glow_fill,
            glow_frame: Frame::default(),
            edge_effect: None,
            edge_started: Instant::now(),
            landing_visible: false,
            timer_running: false,
            glow_bitmap: Bitmap::default(),
            landing_frame: Frame::default(),
            landing_bitmap: Bitmap::default(),
            landing,
            landing_fill,
            landing_started: Instant::now(),
            landing_center: (0.0, 0.0),
            landing_look: Look::default(),
            animate: true,
            hide_landing,
        })
    }

    fn show_glow(
        &mut self,
        display: crate::input::Bounds,
        edge: a7n_proto::v1::Edge,
        progress: u16,
        look: Look,
    ) {
        let screens = screen_frames(self.mtm);
        let Some(primary) = screens.first() else {
            return;
        };
        let top = primary.height - f64::from(display.y);
        let Some(mut screen) = geometry::screen_containing(
            &screens,
            f64::from(display.x) + f64::from(display.width) / 2.0,
            top - f64::from(display.height) / 2.0,
        ) else {
            return;
        };
        // The daemon narrows the display rectangle for a notch push.
        if f64::from(display.width) < screen.width {
            screen.x = f64::from(display.x);
            screen.width = f64::from(display.width);
            screen.height = top - screen.y;
        }
        let rect = if look.style == Style::Beam {
            let pointer = NSEvent::mouseLocation();
            let flipped = match edge {
                a7n_proto::v1::Edge::Top => a7n_proto::v1::Edge::Bottom,
                a7n_proto::v1::Edge::Bottom => a7n_proto::v1::Edge::Top,
                other => other,
            };
            look::beam_rect(
                screen,
                flipped,
                (pointer.x, pointer.y),
                progress,
                BEAM_THICKNESS * look.size.multiplier(),
            )
        } else {
            let mut rect = geometry::glow_rect(screen, edge, progress, look.size);
            if look.style.animated() {
                let thickness = (f64::from(raster::EDGE_DEPTH) * look.size.multiplier())
                    .min(screen.width)
                    .min(screen.height);
                rect = rect.map(|mut rect| {
                    match edge {
                        a7n_proto::v1::Edge::Top => {
                            rect.height = thickness;
                            rect.y = screen.y + screen.height - thickness;
                        }
                        a7n_proto::v1::Edge::Bottom => rect.height = thickness,
                        a7n_proto::v1::Edge::Left => rect.width = thickness,
                        a7n_proto::v1::Edge::Right => {
                            rect.width = thickness;
                            rect.x = screen.x + screen.width - thickness;
                        }
                        a7n_proto::v1::Edge::Unspecified => {}
                    }
                    rect
                });
            }
            rect
        };
        let Some(rect) = rect else {
            self.edge_effect = None;
            self.glow.orderOut(None);
            self.arm_timer();
            return;
        };
        self.glow.setFrame_display(ns_rect(rect), false);
        let scale = self.glow.backingScaleFactor();
        self.glow_frame.resize(
            pixel_size(rect.width * scale),
            pixel_size(rect.height * scale),
        );
        self.glow_frame.dark = true;
        self.glow_frame.opacity = if look.style == Style::Beam { 1.0 } else { 0.65 };
        let pointer = NSEvent::mouseLocation();
        let horizontal = matches!(edge, a7n_proto::v1::Edge::Top | a7n_proto::v1::Edge::Bottom);
        let along = Along {
            start: if horizontal {
                (rect.x - screen.x) * scale
            } else {
                (screen.y + screen.height - rect.y - rect.height) * scale
            },
            span: if horizontal {
                screen.width * scale
            } else {
                screen.height * scale
            },
            push: if horizontal {
                (pointer.x - rect.x) * scale
            } else {
                (rect.y + rect.height - pointer.y) * scale
            },
        };
        self.glow_frame.animate = !reduce_motion();
        self.edge_effect = Some(EdgeEffect {
            look,
            edge,
            progress,
            along,
        });
        self.render_edge();
        self.arm_timer();
        self.glow.orderFrontRegardless();
    }

    fn show_landing(&mut self, x: i32, y: i32, look: Look) {
        let primary_height = screen_frames(self.mtm)
            .first()
            .map_or(0.0, |frame| frame.height);
        let rect = geometry::landing_rect(x, y, primary_height);
        self.show_landing_at(
            (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
            look,
        );
    }

    fn show_landing_at(&mut self, center: (f64, f64), look: Look) {
        self.landing_center = center;
        self.landing_started = Instant::now();
        self.landing_look = look;
        self.animate = !reduce_motion();
        self.landing_visible = true;
        self.fade_landing();
        self.landing.orderFrontRegardless();
        self.arm_timer();
    }

    fn render_edge(&mut self) {
        if let Some(effect) = self.edge_effect {
            effect.render(&mut self.glow_frame, self.edge_started.elapsed());
            self.glow_bitmap.show(&self.glow_fill, &self.glow_frame);
        }
    }

    fn animate_frame(&mut self) {
        if self
            .edge_effect
            .is_some_and(|effect| effect.look.style.animated())
            && self.glow_frame.animate
        {
            self.render_edge();
        }
        if self.landing_visible {
            self.fade_landing();
        }
        self.arm_timer();
    }

    fn arm_timer(&mut self) {
        let edge = self
            .edge_effect
            .is_some_and(|effect| effect.look.style.animated())
            && self.glow_frame.animate;
        let running = edge || (self.landing_visible && self.animate);
        if running && self.timer_running {
            return;
        }
        self.timer_running = running;
        let delay = if running {
            LANDING_FRAME_SECONDS
        } else if self.landing_visible {
            LANDING_STATIC_HOLD
                .saturating_sub(self.landing_started.elapsed())
                .as_secs_f64()
        } else {
            TIMER_DISARMED_SECONDS
        };
        self.hide_landing
            .set_next_fire_date(CFAbsoluteTimeGetCurrent() + delay);
    }

    fn fade_landing(&mut self) {
        let look = self.landing_look;
        let duration = if self.animate {
            LANDING_FADE
        } else {
            LANDING_STATIC_HOLD
        };
        let (_, alpha) = look
            .landing
            .frame(self.landing_started.elapsed(), duration, self.animate);
        if alpha == 0.0 {
            self.landing.orderOut(None);
            self.landing_visible = false;
            return;
        }
        let radius = LANDING_DIAMETER / 2.0 * look.size.multiplier();
        self.landing.setFrame_display(
            ns_rect(Rect {
                x: self.landing_center.0 - radius,
                y: self.landing_center.1 - radius,
                width: radius * 2.0,
                height: radius * 2.0,
            }),
            false,
        );
        let scale = self.landing.backingScaleFactor();
        self.landing_frame.resize(
            pixel_size(radius * 2.0 * scale),
            pixel_size(radius * 2.0 * scale),
        );
        self.landing_frame.animate = self.animate;
        self.landing_frame.dark = true;
        self.landing_frame.opacity = 1.0;
        self.landing_frame.stroke = LANDING_RING_WIDTH * look.size.multiplier() * scale;
        raster::render_landing(
            &mut self.landing_frame,
            look,
            self.landing_started.elapsed(),
            duration,
        );
        self.landing_bitmap
            .show(&self.landing_fill, &self.landing_frame);
    }
}

/// A borderless panel that cannot become key, ignores the mouse, floats over
/// every Space and full-screen app, and is excluded from screen capture.
fn make_panel(mtm: MainThreadMarker, content: &NSImageView) -> Retained<NSPanel> {
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        mtm.alloc::<NSPanel>(),
        NSRect::ZERO,
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    panel.setIgnoresMouseEvents(true);
    panel.setLevel(NSStatusWindowLevel);
    panel.setSharingType(NSWindowSharingType::None);
    panel.setHidesOnDeactivate(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setContentView(Some(content));
    panel
}

fn image_view(mtm: MainThreadMarker) -> Retained<NSImageView> {
    NSImageView::initWithFrame(mtm.alloc::<NSImageView>(), NSRect::ZERO)
}

// Screen dimensions are finite, positive OS values in points.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pixel_size(value: f64) -> u32 {
    value.round().max(1.0) as u32
}

fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

fn tick_haptics() {
    NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern_performanceTime(
        NSHapticFeedbackPattern::Alignment,
        NSHapticFeedbackPerformanceTime::Now,
    );
}

/// All screen frames in `AppKit` space, the primary screen first.
fn screen_frames(mtm: MainThreadMarker) -> Vec<Rect> {
    NSScreen::screens(mtm)
        .iter()
        .map(|screen| {
            let frame = screen.frame();
            Rect {
                x: frame.origin.x,
                y: frame.origin.y,
                width: frame.size.width,
                height: frame.size.height,
            }
        })
        .collect()
}

fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}
