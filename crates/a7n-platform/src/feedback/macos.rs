// SPDX-License-Identifier: GPL-3.0-or-later

//! Edge glow, landing ring and haptic ticks on macOS; contract in [`super`].
//!
//! Two click-through, non-activating panels are owned by one UI thread that
//! runs a `CFRunLoop`: a strip along the pushed edge and a ring around the
//! landing point. A bridge thread blocks on the tokio receiver and wakes the
//! run loop through a `CFRunLoopSource` when commands arrive, so nothing polls.
//!
//! `AppKit` expects windows on the process main thread, which the daemon keeps
//! for tokio. The UI thread therefore asserts a `MainThreadMarker` itself and
//! relies on the same dedicated-run-loop approach as the clipboard backend.
//! Only plain panels are used (no delegates, no events, no key status).

use std::ffi::c_void;
use std::sync::mpsc as sync;
use std::thread;
use std::time::Duration;

use objc2::MainThreadMarker;
use objc2::rc::{Retained, autoreleasepool};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSApplication,
    NSApplicationActivationPolicy, NSBackingStoreType, NSBox, NSBoxType, NSColor, NSEvent,
    NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer, NSPanel, NSScreen, NSStatusWindowLevel, NSTitlePosition,
    NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopSource, CFRunLoopSourceContext,
    CFRunLoopTimer, CFRunLoopTimerContext, kCFRunLoopDefaultMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tokio::sync::mpsc;

use super::macos_geometry::{self as geometry, LANDING_DIAMETER, Rect};
use super::{Command, Feedback};

const GLOW_ALPHA: f64 = 0.65;
const LANDING_RING_WIDTH: f64 = 4.0;
/// Long enough to notice, short enough not to linger over the new window.
const LANDING_FADE: Duration = Duration::from_millis(700);
/// With Reduce Motion on the ring stays put instead of fading.
const LANDING_STATIC_HOLD: Duration = Duration::from_millis(500);
/// Never fires on its own; every landing re-arms it.
const TIMER_DISARMED_SECONDS: f64 = 1.0e12;
const TIMER_NO_INTERVAL: f64 = 0.0;

/// Everything the UI thread owns. Only touched from its run-loop callbacks.
struct Ui {
    mtm: MainThreadMarker,
    commands: sync::Receiver<Command>,
    glow: Retained<NSPanel>,
    landing: Retained<NSPanel>,
    landing_timer: Option<CFRetained<CFRunLoopTimer>>,
}

/// Signals the UI thread's run loop from the bridge thread.
struct Waker {
    source: CFRetained<CFRunLoopSource>,
    run_loop: CFRetained<CFRunLoop>,
}

// SAFETY: CFRunLoopSourceSignal and CFRunLoopWakeUp are documented as safe to
// call from any thread; the Waker exposes nothing else.
unsafe impl Send for Waker {}

impl Waker {
    fn wake(&self) {
        self.source.signal();
        self.run_loop.wake_up();
    }
}

pub(super) fn start() -> Option<Feedback> {
    let (feedback, receiver) = super::channel();
    let (command_tx, command_rx) = sync::channel();
    let (ready_tx, ready_rx) = sync::channel();
    thread::Builder::new()
        .name("a7n-macos-feedback".into())
        .spawn(move || autoreleasepool(|_| run_ui(command_rx, &ready_tx)))
        .ok()?;
    let waker = ready_rx.recv().ok()??;
    thread::Builder::new()
        .name("a7n-macos-feedback-bridge".into())
        .spawn(move || forward_commands(receiver, &command_tx, &waker))
        .ok()?;
    Some(feedback)
}

fn forward_commands(
    mut receiver: mpsc::Receiver<Command>,
    commands: &sync::Sender<Command>,
    waker: &Waker,
) {
    while let Some(command) = receiver.blocking_recv() {
        if commands.send(command).is_err() {
            return;
        }
        waker.wake();
    }
    // Waking with the sender gone tells the UI thread to stop.
    waker.wake();
}

fn run_ui(commands: sync::Receiver<Command>, ready: &sync::Sender<Option<Waker>>) {
    let mut ui = Ui::new(commands);
    let Some((waker, timer)) = install_run_loop_sources(&mut ui) else {
        let _ = ready.send(None);
        return;
    };
    ui.landing_timer = Some(timer);
    if ready.send(Some(waker)).is_ok() {
        CFRunLoop::run();
    }
    if let Some(timer) = ui.landing_timer.take() {
        timer.invalidate();
    }
    ui.glow.orderOut(None);
    ui.landing.orderOut(None);
}

/// Adds the wake-up source and the landing timer; both call back with `ui`,
/// which outlives them because `run_ui` invalidates them before returning.
fn install_run_loop_sources(ui: &mut Ui) -> Option<(Waker, CFRetained<CFRunLoopTimer>)> {
    let run_loop = CFRunLoop::current()?;
    let info: *mut c_void = std::ptr::from_mut(ui).cast();
    let mut source_context = CFRunLoopSourceContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copyDescription: None,
        equal: None,
        hash: None,
        schedule: None,
        cancel: None,
        perform: Some(drain_commands),
    };
    // SAFETY: The default allocator accepts None, the context is valid for
    // the call and `info` points to the Ui that outlives the source.
    let source = unsafe { CFRunLoopSource::new(None, 0, &raw mut source_context) }?;
    let mut timer_context = CFRunLoopTimerContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copyDescription: None,
    };
    // SAFETY: As above; `hide_landing` is the callback this timer expects.
    let timer = unsafe {
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
    let mode = unsafe { kCFRunLoopDefaultMode };
    run_loop.add_source(Some(&source), mode);
    run_loop.add_timer(Some(&timer), mode);
    Some((Waker { source, run_loop }, timer))
}

unsafe extern "C-unwind" fn drain_commands(info: *mut c_void) {
    autoreleasepool(|_| {
        // SAFETY: install_run_loop_sources passes a live, exclusively owned Ui;
        // run-loop callbacks run one at a time on the thread that owns it.
        let ui = unsafe { &mut *info.cast::<Ui>() };
        loop {
            match ui.commands.try_recv() {
                Ok(command) => ui.apply(command),
                Err(sync::TryRecvError::Empty) => return,
                Err(sync::TryRecvError::Disconnected) => {
                    if let Some(run_loop) = CFRunLoop::current() {
                        run_loop.stop();
                    }
                    return;
                }
            }
        }
    });
}

unsafe extern "C-unwind" fn hide_landing(_: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: Same contract as drain_commands.
    let ui = unsafe { &*info.cast::<Ui>() };
    ui.landing.orderOut(None);
}

impl Ui {
    fn new(commands: sync::Receiver<Command>) -> Self {
        // SAFETY: AppKit prefers the main thread, which tokio occupies in the
        // daemon. The marker is only used to create plain panels and read
        // screen frames on this thread, which owns them for its lifetime.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let app = NSApplication::sharedApplication(mtm);
        // Without this a plain process would get a Dock icon and Cmd-Tab entry.
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let accent = NSColor::controlAccentColor();
        let glow = make_panel(
            mtm,
            &filled_box(mtm, &accent.colorWithAlphaComponent(GLOW_ALPHA)),
        );
        let ring = ring_box(mtm, &accent);
        let landing = make_panel(mtm, &ring);
        Self {
            mtm,
            commands,
            glow,
            landing,
            landing_timer: None,
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Edge { edge, progress, .. } => self.show_glow(edge, progress),
            Command::Clear => self.glow.orderOut(None),
            Command::Landing { x, y } => self.show_landing(x, y),
            Command::Tick => tick_haptics(),
        }
    }

    fn show_glow(&self, edge: a7n_proto::v1::Edge, progress: u16) {
        let screens = screen_frames(self.mtm);
        let pointer = NSEvent::mouseLocation();
        let Some(screen) = geometry::screen_containing(&screens, pointer.x, pointer.y) else {
            return;
        };
        let Some(rect) = geometry::glow_rect(screen, edge, progress) else {
            self.glow.orderOut(None);
            return;
        };
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
        if let Some(timer) = &self.landing_timer {
            timer.set_next_fire_date(CFAbsoluteTimeGetCurrent() + visible_for.as_secs_f64());
        }
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
