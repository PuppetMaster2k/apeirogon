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

use std::ffi::c_void;
use std::sync::mpsc as sync;
use std::thread;
use std::time::Duration;

use objc2::MainThreadMarker;
use objc2::rc::{Retained, autoreleasepool};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSBackingStoreType, NSBox, NSBoxType,
    NSColor, NSEvent, NSHapticFeedbackManager, NSHapticFeedbackPattern,
    NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer, NSPanel, NSScreen,
    NSStatusWindowLevel, NSTitlePosition, NSWindowCollectionBehavior, NSWindowSharingType,
    NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext,
    kCFRunLoopDefaultMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tokio::sync::mpsc;

use super::look::{self, BEAM_THICKNESS};
use super::macos_geometry::{self as geometry, LANDING_DIAMETER, Rect};
use super::{Color, Command, Feedback, Look, Style};
use crate::main_thread::{self, Waker};

const GLOW_ALPHA: f64 = 0.65;
const LANDING_RING_WIDTH: f64 = 4.0;
/// Long enough to notice, short enough not to linger over the new window.
const LANDING_FADE: Duration = Duration::from_millis(700);
/// With Reduce Motion on the ring stays put instead of fading.
const LANDING_STATIC_HOLD: Duration = Duration::from_millis(500);
/// Never fires on its own; every landing re-arms it.
const TIMER_DISARMED_SECONDS: f64 = 1.0e12;
const TIMER_NO_INTERVAL: f64 = 0.0;

/// State of the main-thread callbacks. Leaked on purpose: it lives as long as
/// the process and is only touched from main run-loop callbacks.
struct Ui {
    commands: sync::Receiver<Command>,
    panels: Option<Panels>,
}

struct Panels {
    mtm: MainThreadMarker,
    glow: Retained<NSPanel>,
    glow_fill: Retained<NSBox>,
    colors: [(Color, Retained<NSColor>); 4],
    landing: Retained<NSPanel>,
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
    let ui = unsafe { &*info.cast::<Ui>() };
    if let Some(panels) = &ui.panels {
        panels.landing.orderOut(None);
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
            Command::Landing { x, y } => {
                if let Some(panels) = self.panels() {
                    panels.show_landing(x, y);
                }
            }
        }
    }

    fn hide_glow(&self) {
        if let Some(panels) = &self.panels {
            panels.glow.orderOut(None);
        }
    }

    fn hide_all(&self) {
        self.hide_glow();
        if let Some(panels) = &self.panels {
            panels.landing.orderOut(None);
            panels.hide_landing.invalidate();
        }
    }

    /// The panels, created on first use; callbacks run on the main thread.
    fn panels(&mut self) -> Option<&Panels> {
        if self.panels.is_none() {
            let info = std::ptr::from_mut(self).cast();
            self.panels = Panels::new(MainThreadMarker::new()?, info);
        }
        self.panels.as_ref()
    }
}

impl Panels {
    fn new(mtm: MainThreadMarker, timer_info: *mut c_void) -> Option<Self> {
        let colors = [Color::Accent, Color::Green, Color::Orange, Color::Pink].map(|color| {
            let [r, g, b] = color.rgb(true);
            (
                color,
                NSColor::colorWithSRGBRed_green_blue_alpha(
                    f64::from(r) / 255.0,
                    f64::from(g) / 255.0,
                    f64::from(b) / 255.0,
                    1.0,
                ),
            )
        });
        let accent = NSColor::controlAccentColor();
        let glow_fill = filled_box(mtm, &accent.colorWithAlphaComponent(GLOW_ALPHA));
        let glow = make_panel(mtm, &glow_fill);
        let landing = make_panel(mtm, &ring_box(mtm, &accent));
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
                TIMER_NO_INTERVAL,
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
            colors,
            landing,
            hide_landing,
        })
    }

    fn show_glow(
        &self,
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
            geometry::glow_rect(screen, edge, progress, look.size)
        };
        let Some(rect) = rect else {
            self.glow.orderOut(None);
            return;
        };
        if let Some((_, color)) = self.colors.iter().find(|(color, _)| *color == look.color) {
            self.glow_fill.setFillColor(color);
        }
        self.glow.setAlphaValue(if look.style == Style::Beam {
            1.0
        } else {
            GLOW_ALPHA
        });
        self.glow.setFrame_display(ns_rect(rect), true);
        self.glow.orderFrontRegardless();
    }

    fn show_landing(&self, x: i32, y: i32) {
        let primary_height = screen_frames(self.mtm)
            .first()
            .map_or(0.0, |frame| frame.height);
        let rect = geometry::landing_rect(x, y, primary_height);
        set_alpha(&self.landing, 1.0, Duration::ZERO);
        self.landing.setFrame_display(ns_rect(rect), true);
        self.landing.orderFrontRegardless();
        let visible_for = if reduce_motion() {
            LANDING_STATIC_HOLD
        } else {
            set_alpha(&self.landing, 0.0, LANDING_FADE);
            LANDING_FADE
        };
        self.hide_landing
            .set_next_fire_date(CFAbsoluteTimeGetCurrent() + visible_for.as_secs_f64());
    }
}

/// A borderless panel that cannot become key, ignores the mouse, floats over
/// every Space and full-screen app, and is excluded from screen capture.
fn make_panel(mtm: MainThreadMarker, content: &NSBox) -> Retained<NSPanel> {
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

fn filled_box(mtm: MainThreadMarker, fill: &NSColor) -> Retained<NSBox> {
    let view = plain_box(mtm);
    view.setBorderWidth(0.0);
    view.setFillColor(fill);
    view
}

fn ring_box(mtm: MainThreadMarker, accent: &NSColor) -> Retained<NSBox> {
    let view = plain_box(mtm);
    view.setBorderWidth(LANDING_RING_WIDTH);
    view.setBorderColor(accent);
    view.setCornerRadius(LANDING_DIAMETER / 2.0);
    view.setFillColor(&NSColor::clearColor());
    view
}

fn plain_box(mtm: MainThreadMarker) -> Retained<NSBox> {
    let view = NSBox::initWithFrame(mtm.alloc::<NSBox>(), NSRect::ZERO);
    view.setBoxType(NSBoxType::Custom);
    view.setTitlePosition(NSTitlePosition::NoTitle);
    view
}

/// Animates the panel's alpha; a zero duration applies it at once.
fn set_alpha(panel: &NSPanel, alpha: f64, duration: Duration) {
    NSAnimationContext::beginGrouping();
    NSAnimationContext::currentContext().setDuration(duration.as_secs_f64());
    panel.animator().setAlphaValue(alpha);
    NSAnimationContext::endGrouping();
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
