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
mod window;

use std::ffi::c_void;
use std::sync::mpsc as sync;
use std::thread;
use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::rc::{Retained, autoreleasepool};
use objc2_app_kit::{
    NSEvent, NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer, NSImageView, NSPanel, NSWorkspace,
};
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext,
    kCFRunLoopDefaultMode,
};
use tokio::sync::mpsc;

use super::look;
use super::macos_geometry::{self as geometry, Rect};
use super::raster::{self, Along, Arrival, EdgeEffect, Frame};
use super::timeline::{End, Timeline, frame_interval};
use super::{Command, Feedback, Look};
use crate::main_thread::{self, Waker};
use blit::Bitmap;
use window::{image_view, make_panel, ns_rect, pixel_size, screen_frames};

/// Never fires on its own; every landing re-arms it.
const TIMER_DISARMED_SECONDS: f64 = 1.0e12;

/// State of the main-thread callbacks. Leaked on purpose: it lives as long as
/// the process and is only touched from main run-loop callbacks.
struct Ui {
    commands: sync::Receiver<Command>,
    panels: Option<Panels>,
}

struct Glow {
    window: Retained<NSPanel>,
    fill: Retained<NSImageView>,
    frame: Frame,
    bitmap: Bitmap,
}

struct LandingMark {
    window: Retained<NSPanel>,
    fill: Retained<NSImageView>,
    frame: Frame,
    bitmap: Bitmap,
    center: (f64, f64),
    look: Look,
}

struct Panels {
    mtm: MainThreadMarker,
    glow: Glow,
    landing: LandingMark,
    timeline: Timeline,
    edge_started: Instant,
    timer_running: bool,
    frame_timer: CFRetained<CFRunLoopTimer>,
    timer_info: *mut c_void,
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

unsafe extern "C-unwind" fn on_frame_timer(_: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: Same contract as drain_commands.
    let ui = unsafe { &mut *info.cast::<Ui>() };
    // The main thread runs a bare CFRunLoop, so nothing else drains objects
    // autoreleased while painting each frame.
    autoreleasepool(|_| {
        if let Some(panels) = &mut ui.panels {
            panels.animate_frame();
        }
    });
}

impl Ui {
    fn apply(&mut self, command: Command) {
        match command {
            Command::Tick => tick_haptics(),
            Command::Clear | Command::Crossed => {
                if let Some(panels) = &mut self.panels {
                    panels
                        .timeline
                        .end(End::for_command(command), panels.edge_started.elapsed());
                    panels.arm_timer();
                }
            }
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
            panels.timeline = Timeline::default();
            panels.glow.window.orderOut(None);
            panels.arm_timer();
        }
    }

    fn hide_all(&mut self) {
        self.hide_glow();
        if let Some(panels) = &self.panels {
            panels.landing.window.orderOut(None);
            panels.frame_timer.invalidate();
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
        let timer = frame_timer(timer_info, frame_interval(None).as_secs_f64())?;
        Some(Self {
            mtm,
            glow: Glow {
                window: glow,
                fill: glow_fill,
                frame: Frame::default(),
                bitmap: Bitmap::default(),
            },
            landing: LandingMark {
                window: landing,
                fill: landing_fill,
                frame: Frame::default(),
                bitmap: Bitmap::default(),
                center: (0.0, 0.0),
                look: Look::default(),
            },
            timeline: Timeline::default(),
            edge_started: Instant::now(),
            timer_running: false,
            frame_timer: timer,
            timer_info,
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
        let pointer = NSEvent::mouseLocation();
        let Some(mut screen) = geometry::screen_containing(&screens, pointer.x, pointer.y) else {
            return;
        };
        // The daemon narrows the display rectangle for a notch push.
        if f64::from(display.width) < screen.width {
            screen.x = f64::from(display.x);
            screen.width = f64::from(display.width);
            screen.height = top - screen.y;
        }
        let rect = geometry::glow_rect(screen, edge, f64::from(raster::OVERLAY_DEPTH));
        let Some(rect) = rect else {
            self.timeline
                .end(End::Abandoned, self.edge_started.elapsed());
            self.glow.window.orderOut(None);
            self.arm_timer();
            return;
        };
        self.paint_glow(rect, screen, edge, progress, look);
    }

    fn paint_glow(
        &mut self,
        rect: Rect,
        screen: Rect,
        edge: a7n_proto::v1::Edge,
        progress: u16,
        look: Look,
    ) {
        self.glow.window.setFrame_display(ns_rect(rect), false);
        let scale = self.glow.window.backingScaleFactor();
        self.glow.frame.prepare(
            pixel_size(rect.width * scale),
            pixel_size(rect.height * scale),
            scale,
            !reduce_motion(),
        );
        let pointer = NSEvent::mouseLocation();
        let along = Along::across(
            Rect {
                y: -screen.y - screen.height,
                ..screen
            },
            Rect {
                y: -rect.y - rect.height,
                ..rect
            },
            (pointer.x, -pointer.y),
            edge,
            scale,
        );
        self.timeline.push(
            EdgeEffect {
                look,
                edge,
                progress,
                along,
            },
            self.edge_started.elapsed(),
        );
        self.render_edge();
        self.arm_timer();
        self.glow.window.orderFrontRegardless();
    }

    fn show_landing(&mut self, x: i32, y: i32, look: Look) {
        let primary_height = screen_frames(self.mtm)
            .first()
            .map_or(0.0, |frame| frame.height);
        self.show_landing_at(geometry::desktop_to_appkit(x, y, primary_height), look);
    }

    fn show_landing_at(&mut self, center: (f64, f64), look: Look) {
        self.landing.center = center;
        self.timeline.land(look, self.edge_started.elapsed());
        self.landing.look = look;
        self.landing.frame.animate = !reduce_motion();
        self.fade_landing();
        self.landing.window.orderFrontRegardless();
        self.arm_timer();
    }

    fn render_edge(&mut self) {
        if self
            .timeline
            .render_edge(&mut self.glow.frame, self.edge_started.elapsed())
        {
            self.glow.bitmap.show(&self.glow.fill, &self.glow.frame);
        } else {
            self.glow.window.orderOut(None);
        }
    }

    fn animate_frame(&mut self) {
        self.render_edge();
        if self.timeline.landing_visible() {
            self.fade_landing();
        }
        self.arm_timer();
    }

    fn arm_timer(&mut self) {
        let running =
            self.timeline.edge_playing(self.glow.frame.animate) || self.timeline.landing_visible();
        if running && self.timer_running {
            return;
        }
        self.timer_running = running;
        let delay = if running {
            self.pace_timer();
            self.frame_timer.interval()
        } else {
            TIMER_DISARMED_SECONDS
        };
        self.frame_timer
            .set_next_fire_date(CFAbsoluteTimeGetCurrent() + delay);
    }

    /// A repeating timer cannot change its interval, so a display with another
    /// refresh rate gets a new timer. `arm_timer` calls it once per effect, when it starts.
    fn pace_timer(&mut self) {
        let seconds = frame_interval(self.refresh_hz()).as_secs_f64();
        if (self.frame_timer.interval() - seconds).abs() < f64::EPSILON {
            return;
        }
        if let Some(timer) = frame_timer(self.timer_info, seconds) {
            self.frame_timer.invalidate();
            self.frame_timer = timer;
        }
    }

    /// The refresh rate of the screen the playing effect is on.
    fn refresh_hz(&self) -> Option<f64> {
        let panel = if self.timeline.landing_visible() {
            &self.landing.window
        } else {
            &self.glow.window
        };
        let frames = panel.screen()?.maximumFramesPerSecond();
        i32::try_from(frames).ok().map(f64::from)
    }

    fn fade_landing(&mut self) {
        let look = self.landing.look;
        let diameter = look.arrival_diameter();
        let point = (self.landing.center.0, -self.landing.center.1);
        let screens = screen_frames(self.mtm);
        let Some(screen) =
            geometry::screen_containing(&screens, self.landing.center.0, self.landing.center.1)
        else {
            return;
        };
        let area = Rect {
            y: -screen.y - screen.height,
            ..screen
        };
        let rect = look::landing_rect(area, point, diameter);
        self.landing.window.setFrame_display(
            ns_rect(Rect {
                y: -rect.y - rect.height,
                ..rect
            }),
            false,
        );
        let scale = self.landing.window.backingScaleFactor();
        self.landing.frame.prepare(
            pixel_size(rect.width * scale),
            pixel_size(rect.height * scale),
            scale,
            self.landing.frame.animate,
        );
        self.landing.frame.arrival = Some(Arrival::at(rect, point, scale));
        if !self
            .timeline
            .render_landing(&mut self.landing.frame, self.edge_started.elapsed())
        {
            self.landing.window.orderOut(None);
            return;
        }
        self.landing
            .bitmap
            .show(&self.landing.fill, &self.landing.frame);
    }
}

/// The repeating frame timer on the main run loop; `info` is the leaked [`Ui`].
fn frame_timer(info: *mut c_void, seconds: f64) -> Option<CFRetained<CFRunLoopTimer>> {
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copyDescription: None,
    };
    // SAFETY: The default allocator accepts None; `info` is the leaked Ui
    // that `on_frame_timer` expects and the context is copied.
    let timer = unsafe {
        CFRunLoopTimer::new(
            None,
            CFAbsoluteTimeGetCurrent() + TIMER_DISARMED_SECONDS,
            seconds,
            0,
            0,
            Some(on_frame_timer),
            &raw mut context,
        )
    }?;
    // SAFETY: Core Foundation's default run-loop mode is an immutable global.
    CFRunLoop::current()?.add_timer(Some(&timer), unsafe { kCFRunLoopDefaultMode });
    Some(timer)
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
