// SPDX-License-Identifier: GPL-3.0-or-later

//! Edge glow and landing marker as click-through layered windows.
//!
//! One UI thread owns the window class, both windows and the message loop. A
//! relay thread waits on the command channel and posts each command to the UI
//! thread as a thread message, so nothing busy-waits and the UI thread never
//! touches tokio.

mod blit;
mod geometry;

use std::ptr::{null, null_mut};
use std::sync::mpsc as sync;
use std::thread;
use std::time::Instant;

use a7n_proto::v1::Edge;
use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetThreadDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer, MSG,
    PostThreadMessageW, RegisterClassW, SPI_GETCLIENTAREAANIMATION, SW_HIDE, SetTimer,
    SetWindowDisplayAffinity, ShowWindow, SystemParametersInfoW, WDA_EXCLUDEFROMCAPTURE, WM_APP,
    WM_QUIT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_sys::core::{BOOL, PCWSTR, w};

use self::geometry::{LANDING_DURATION, Rect, glow_alpha, glow_rect, landing_alpha};
use super::look::{self, BEAM_THICKNESS};
use super::raster::{self, Along, EdgeEffect, Frame};
use super::{Command, Feedback, Look, Style};
use blit::Bitmap;

/// Carries a boxed [`Command`] in `lParam` from the relay to the UI thread.
const WM_FEEDBACK: u32 = WM_APP;
const CLASS_NAME: PCWSTR = w!("ApeirogonFeedback");
const BASE_DPI: u32 = 96;
const GLOW_THICKNESS_DIP: i32 = 6;
const LANDING_DIAMETER_DIP: i32 = 56;
const LANDING_RING_DIP: i32 = 5;
const FADE_TIMER_ID: usize = 1;
const EDGE_TIMER_ID: usize = 2;
const FADE_FRAME_MS: u32 = 16;

pub(super) fn start() -> Option<Feedback> {
    let (feedback, commands) = super::channel();
    let (ready_tx, ready_rx) = sync::channel();
    thread::Builder::new()
        .name("a7n-windows-feedback".into())
        .spawn(move || run_ui_thread(&ready_tx))
        .ok()?;
    let ui_thread = ready_rx.recv().ok()??;
    let relay = thread::Builder::new()
        .name("a7n-windows-feedback-relay".into())
        .spawn(move || relay_commands(commands, ui_thread));
    if relay.is_err() {
        post_quit(ui_thread);
        return None;
    }
    Some(feedback)
}

fn relay_commands(mut commands: mpsc::Receiver<Command>, ui_thread: u32) {
    while let Some(command) = commands.blocking_recv() {
        let raw = Box::into_raw(Box::new(command));
        // SAFETY: The UI thread takes ownership of `raw` from lParam when the
        // message is delivered; nothing else reads it.
        if unsafe { PostThreadMessageW(ui_thread, WM_FEEDBACK, 0, raw as LPARAM) } == 0 {
            // SAFETY: The post failed, so `raw` was never handed over and is
            // still the unique owner of the allocation made above.
            drop(unsafe { Box::from_raw(raw) });
        }
    }
    post_quit(ui_thread);
}

fn post_quit(ui_thread: u32) {
    // SAFETY: Posts a pointer-free quit to the UI thread; failure means it is gone.
    unsafe { PostThreadMessageW(ui_thread, WM_QUIT, 0, 0) };
}

fn run_ui_thread(ready: &sync::Sender<Option<u32>>) {
    // SAFETY: Only changes this thread's DPI mode; failure keeps the default,
    // which merely scales the strips.
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let Some(mut ui) = Ui::new() else {
        let _ = ready.send(None);
        return;
    };
    // SAFETY: Reads this thread's id; the windows above gave it a message queue.
    let thread_id = unsafe { GetCurrentThreadId() };
    if ready.send(Some(thread_id)).is_ok() {
        ui.run();
    }
}

struct Ui {
    glow: HWND,
    glow_frame: Frame,
    edge_effect: Option<EdgeEffect>,
    glow_rect: Option<Rect>,
    edge_started: Instant,
    edge_timer: bool,
    glow_bitmap: Bitmap,
    landing_frame: Frame,
    landing_bitmap: Bitmap,
    landing: HWND,
    landing_started: Instant,
    animate: bool,
    landing_look: Look,
    landing_point: (i32, i32),
    landing_dpi: u32,
}

impl Ui {
    fn new() -> Option<Self> {
        // SAFETY: Requests the current module, which outlives the window class.
        let module = unsafe { GetModuleHandleW(null()) };
        register_class(module);
        Some(Self {
            glow: create_overlay(module)?,
            glow_frame: Frame::default(),
            edge_effect: None,
            glow_rect: None,
            edge_started: Instant::now(),
            edge_timer: false,
            glow_bitmap: Bitmap::default(),
            landing_frame: Frame::default(),
            landing_bitmap: Bitmap::default(),
            landing: create_overlay(module)?,
            landing_started: Instant::now(),
            animate: true,
            landing_look: Look::default(),
            landing_point: (0, 0),
            landing_dpi: BASE_DPI,
        })
    }

    fn run(&mut self) {
        let mut message = MSG::default();
        // SAFETY: message is writable storage; this thread owns the message queue.
        while unsafe { GetMessageW(&raw mut message, null_mut(), 0, 0) } > 0 {
            if message.hwnd.is_null() && message.message == WM_FEEDBACK {
                // SAFETY: Only `relay_commands` posts WM_FEEDBACK to this
                // thread, always with a pointer from `Box::into_raw`.
                let command = *unsafe { Box::from_raw(message.lParam as *mut Command) };
                self.handle(command);
            } else if message.hwnd == self.landing
                && message.message == WM_TIMER
                && message.wParam == FADE_TIMER_ID
            {
                self.fade_landing();
            } else if message.hwnd == self.glow
                && message.message == WM_TIMER
                && message.wParam == EDGE_TIMER_ID
            {
                self.animate_edge();
            } else {
                // SAFETY: message was initialized by GetMessageW.
                unsafe { DispatchMessageW(&raw const message) };
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Edge {
                display,
                edge,
                progress,
                look,
            } => self.show_glow(Rect::from_bounds(display), edge, progress, look),
            Command::Clear => {
                self.edge_effect = None;
                self.edge_timer(false);
                hide(self.glow);
            }
            Command::Landing { x, y, look } => self.show_landing((x, y), look),
            Command::LandingAtCursor { look } => {
                if let Some(point) = cursor_position() {
                    self.show_landing(point, look);
                }
            }
            Command::Tick => {}
        }
    }

    fn show_glow(&mut self, display: Rect, edge: Edge, progress: u16, look: Look) {
        let pointer = cursor_position().unwrap_or_else(|| center(display));
        let monitor = monitor_at(pointer);
        let area = monitor
            .and_then(|monitor| monitor.rect.intersect(display))
            .unwrap_or(display);
        let thickness = scale_dip(GLOW_THICKNESS_DIP, monitor.map_or(BASE_DPI, |m| m.dpi));
        let thickness = i32::try_from(look.size.pixels(u32::try_from(thickness).unwrap_or(1)))
            .unwrap_or(thickness);
        let rect = if look.style == Style::Beam {
            let area = look::Rect {
                x: f64::from(area.left),
                y: f64::from(area.top),
                width: f64::from(area.width()),
                height: f64::from(area.height()),
            };
            let dpi = monitor.map_or(BASE_DPI, |m| m.dpi);
            look::beam_rect(
                area,
                edge,
                (f64::from(pointer.0), f64::from(pointer.1)),
                progress,
                BEAM_THICKNESS * f64::from(dpi) / f64::from(BASE_DPI) * look.size.multiplier(),
            )
            .map(|rect| Rect::from_bounds(rect.bounds()))
        } else if look.style.animated() {
            let depth = scale_dip(
                i32::try_from(raster::EDGE_DEPTH).unwrap_or(1),
                monitor.map_or(BASE_DPI, |m| m.dpi),
            );
            let depth =
                i32::try_from(look.size.pixels(u32::try_from(depth).unwrap_or(1))).unwrap_or(depth);
            glow_rect(
                area,
                edge,
                1000,
                depth.min(area.width()).min(area.height()),
                pointer,
            )
        } else {
            glow_rect(area, edge, progress, thickness, pointer)
        };
        let Some(rect) = rect else {
            self.edge_timer(false);
            self.edge_effect = None;
            hide(self.glow);
            return;
        };
        self.glow_frame.resize(
            u32::try_from(rect.width()).unwrap_or(1),
            u32::try_from(rect.height()).unwrap_or(1),
        );
        self.glow_frame.dark = true;
        self.glow_frame.opacity = if look.style == Style::Beam {
            1.0
        } else {
            f64::from(glow_alpha(progress)) / 255.0
        };
        let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
        let along = Along {
            start: f64::from(if horizontal {
                rect.left - area.left
            } else {
                rect.top - area.top
            }),
            span: f64::from(if horizontal {
                area.width()
            } else {
                area.height()
            }),
            push: f64::from(if horizontal {
                pointer.0 - rect.left
            } else {
                pointer.1 - rect.top
            }),
        };
        self.glow_frame.animate = animations_enabled();
        self.edge_effect = Some(EdgeEffect {
            look,
            edge,
            progress,
            along,
        });
        self.animate_edge_at(rect);
        self.edge_timer(look.style.animated() && self.glow_frame.animate);
    }

    fn edge_timer(&mut self, running: bool) {
        if self.edge_timer == running {
            return;
        }
        self.edge_timer = running;
        // SAFETY: This thread owns the glow window and its timer.
        unsafe {
            if running {
                SetTimer(self.glow, EDGE_TIMER_ID, FADE_FRAME_MS, None);
            } else {
                KillTimer(self.glow, EDGE_TIMER_ID);
            }
        }
    }

    fn animate_edge(&mut self) {
        if let Some(rect) = self.glow_rect {
            self.animate_edge_at(rect);
        }
    }

    fn animate_edge_at(&mut self, rect: Rect) {
        self.glow_rect = Some(rect);
        if let Some(effect) = self.edge_effect {
            effect.render(&mut self.glow_frame, self.edge_started.elapsed());
            self.glow_bitmap.show(self.glow, rect, &self.glow_frame);
        }
    }

    fn show_landing(&mut self, point: (i32, i32), look: Look) {
        self.landing_dpi = monitor_at(point).map_or(BASE_DPI, |monitor| monitor.dpi);
        self.landing_point = point;
        self.landing_look = look;
        self.animate = animations_enabled();
        self.landing_started = Instant::now();
        self.fade_landing();
        // SAFETY: The window belongs to this thread; the timer is killed in
        // `fade_landing` and replaced if a new landing arrives first.
        unsafe { SetTimer(self.landing, FADE_TIMER_ID, FADE_FRAME_MS, None) };
    }

    fn fade_landing(&mut self) {
        let alpha = landing_alpha(self.landing_started.elapsed(), self.animate);
        if alpha == 0 {
            // SAFETY: Stops the timer set on this thread's landing window.
            unsafe { KillTimer(self.landing, FADE_TIMER_ID) };
            hide(self.landing);
            return;
        }
        let look = self.landing_look;
        let diameter = look
            .size
            .pixels(u32::try_from(scale_dip(LANDING_DIAMETER_DIP, self.landing_dpi)).unwrap_or(1));
        let half = i32::try_from(diameter / 2).unwrap_or(0);
        let rect = Rect::from_bounds(crate::input::Bounds {
            x: self.landing_point.0 - half,
            y: self.landing_point.1 - half,
            width: diameter,
            height: diameter,
        });
        self.landing_frame.resize(diameter, diameter);
        self.landing_frame.dark = true;
        self.landing_frame.animate = self.animate;
        self.landing_frame.opacity = 220.0 / 255.0;
        self.landing_frame.stroke =
            f64::from(scale_dip(LANDING_RING_DIP, self.landing_dpi)) * look.size.multiplier();
        raster::render_landing(
            &mut self.landing_frame,
            look,
            self.landing_started.elapsed(),
            LANDING_DURATION,
        );
        self.landing_bitmap
            .show(self.landing, rect, &self.landing_frame);
    }
}

fn register_class(module: HINSTANCE) {
    let class = WNDCLASSW {
        lpfnWndProc: Some(overlay_proc),
        hInstance: module,
        lpszClassName: CLASS_NAME,
        ..WNDCLASSW::default()
    };
    // SAFETY: class and its static name/callback are valid. Registration is
    // process-wide and may already exist from an earlier start; a real failure
    // shows up as a failed window creation.
    unsafe { RegisterClassW(&raw const class) };
}

/// A hidden, click-through, topmost window that never takes focus and is
/// excluded from screen capture where the OS supports it.
fn create_overlay(module: HINSTANCE) -> Option<HWND> {
    let style =
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST;
    // SAFETY: The class is registered with a static window procedure; no
    // creation data is passed.
    let hwnd = unsafe {
        CreateWindowExW(
            style,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            module,
            null(),
        )
    };
    if hwnd.is_null() {
        return None;
    }
    // SAFETY: hwnd is live; older Windows versions refuse the flag, which only
    // means the strip can appear in captures.
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) };
    Some(hwnd)
}

fn hide(hwnd: HWND) {
    // SAFETY: hwnd is a live window of this thread.
    unsafe { ShowWindow(hwnd, SW_HIDE) };
}

#[derive(Clone, Copy)]
struct MonitorGeometry {
    rect: Rect,
    dpi: u32,
}

fn monitor_at(point: (i32, i32)) -> Option<MonitorGeometry> {
    // SAFETY: Plain value lookup; DEFAULTTONEAREST always returns a monitor.
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: point.0,
                y: point.1,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).ok()?,
        ..MONITORINFO::default()
    };
    // SAFETY: info is writable and its size field is set.
    if unsafe { GetMonitorInfoW(monitor, &raw mut info) } == 0 {
        return None;
    }
    let (mut dpi_x, mut dpi_y) = (BASE_DPI, BASE_DPI);
    // SAFETY: Both outputs are writable; on failure they keep the base value.
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y) };
    Some(MonitorGeometry {
        rect: Rect {
            left: info.rcMonitor.left,
            top: info.rcMonitor.top,
            right: info.rcMonitor.right,
            bottom: info.rcMonitor.bottom,
        },
        dpi: dpi_x,
    })
}

fn cursor_position() -> Option<(i32, i32)> {
    let mut point = POINT::default();
    // SAFETY: point is writable storage.
    (unsafe { GetCursorPos(&raw mut point) } != 0).then_some((point.x, point.y))
}

fn center(area: Rect) -> (i32, i32) {
    (area.left + area.width() / 2, area.top + area.height() / 2)
}

fn scale_dip(dip: i32, dpi: u32) -> i32 {
    dip * i32::try_from(dpi).unwrap_or(i32::MAX) / i32::try_from(BASE_DPI).unwrap_or(i32::MAX)
}

/// The Windows "Animation effects" setting; assume on when it cannot be read.
fn animations_enabled() -> bool {
    let mut enabled: BOOL = 1;
    // SAFETY: The call writes one BOOL into the provided storage.
    let read = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0)
    };
    read == 0 || enabled != 0
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Forward untouched window messages to the system default procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
