// SPDX-License-Identifier: GPL-3.0-or-later

//! Edge glow and landing marker as click-through layered windows.
//!
//! One UI thread owns the window class, both windows and the message loop. A
//! relay thread waits on the command channel and posts each command to the UI
//! thread as a thread message, so nothing busy-waits and the UI thread never
//! touches tokio.

mod geometry;

use std::ptr::{null, null_mut};
use std::sync::mpsc as sync;
use std::thread;
use std::time::{Duration, Instant};

use a7n_proto::v1::Edge;
use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, POINT};
use windows_sys::Win32::Graphics::Gdi::{
    CombineRgn, CreateEllipticRgn, CreateSolidBrush, DeleteObject, GetMonitorInfoW,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, RGN_DIFF, SetWindowRgn,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetThreadDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, HWND_TOPMOST,
    KillTimer, LWA_ALPHA, MSG, PostThreadMessageW, RegisterClassW, SPI_GETCLIENTAREAANIMATION,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetLayeredWindowAttributes, SetTimer,
    SetWindowDisplayAffinity, SetWindowPos, ShowWindow, SystemParametersInfoW,
    WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_QUIT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_sys::core::{BOOL, PCWSTR, w};

use self::geometry::{Rect, glow_alpha, glow_rect, landing_alpha};
use super::{Command, Feedback};

/// Carries a boxed [`Command`] in `lParam` from the relay to the UI thread.
const WM_FEEDBACK: u32 = WM_APP;
const CLASS_NAME: PCWSTR = w!("ApeirogonFeedback");
/// The GUI accent `#2F5BEA` as a `COLORREF`, which stores blue first.
const ACCENT: COLORREF = 0x00EA_5B2F;
const BASE_DPI: u32 = 96;
const GLOW_THICKNESS_DIP: i32 = 6;
const LANDING_DIAMETER_DIP: i32 = 56;
const LANDING_RING_DIP: i32 = 5;
const FADE_TIMER_ID: usize = 1;
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
    landing: HWND,
    landing_started: Instant,
    animate: bool,
}

impl Ui {
    fn new() -> Option<Self> {
        // SAFETY: Requests the current module, which outlives the window class.
        let module = unsafe { GetModuleHandleW(null()) };
        register_class(module);
        Some(Self {
            glow: create_overlay(module)?,
            landing: create_overlay(module)?,
            landing_started: Instant::now(),
            animate: true,
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
            } => self.show_glow(Rect::from_bounds(display), edge, progress),
            Command::Clear => hide(self.glow),
            Command::Landing { x, y } => self.show_landing((x, y)),
            Command::Tick => {}
        }
    }

    fn show_glow(&self, display: Rect, edge: Edge, progress: u16) {
        let pointer = cursor_position().unwrap_or_else(|| center(display));
        let monitor = monitor_at(pointer);
        let area = monitor
            .and_then(|monitor| monitor.rect.intersect(display))
            .unwrap_or(display);
        let thickness = scale_dip(GLOW_THICKNESS_DIP, monitor.map_or(BASE_DPI, |m| m.dpi));
        match glow_rect(area, edge, progress, thickness, pointer) {
            Some(rect) => show(self.glow, rect, glow_alpha(progress)),
            None => hide(self.glow),
        }
    }

    fn show_landing(&mut self, point: (i32, i32)) {
        let dpi = monitor_at(point).map_or(BASE_DPI, |monitor| monitor.dpi);
        let diameter = scale_dip(LANDING_DIAMETER_DIP, dpi);
        let rect = Rect {
            left: point.0 - diameter / 2,
            top: point.1 - diameter / 2,
            right: point.0 - diameter / 2 + diameter,
            bottom: point.1 - diameter / 2 + diameter,
        };
        set_ring_shape(self.landing, diameter, scale_dip(LANDING_RING_DIP, dpi));
        self.animate = animations_enabled();
        self.landing_started = Instant::now();
        show(
            self.landing,
            rect,
            landing_alpha(Duration::ZERO, self.animate),
        );
        // SAFETY: The window belongs to this thread; the timer is killed in
        // `fade_landing` and replaced if a new landing arrives first.
        unsafe { SetTimer(self.landing, FADE_TIMER_ID, FADE_FRAME_MS, None) };
    }

    fn fade_landing(&self) {
        let alpha = landing_alpha(self.landing_started.elapsed(), self.animate);
        if alpha == 0 {
            // SAFETY: Stops the timer set on this thread's landing window.
            unsafe { KillTimer(self.landing, FADE_TIMER_ID) };
            hide(self.landing);
            return;
        }
        set_alpha(self.landing, alpha);
    }
}

fn register_class(module: HINSTANCE) {
    // SAFETY: Creates a brush owned by the class for the process lifetime.
    let accent = unsafe { CreateSolidBrush(ACCENT) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(DefWindowProcW),
        hInstance: module,
        lpszClassName: CLASS_NAME,
        hbrBackground: accent,
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
    set_alpha(hwnd, 0);
    // SAFETY: hwnd is live; older Windows versions refuse the flag, which only
    // means the strip can appear in captures.
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) };
    Some(hwnd)
}

fn show(hwnd: HWND, rect: Rect, alpha: u8) {
    // SAFETY: hwnd is a live window of this thread; NOACTIVATE keeps focus.
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            SWP_NOACTIVATE,
        );
    }
    set_alpha(hwnd, alpha);
    // SAFETY: hwnd is a live window of this thread.
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
}

fn hide(hwnd: HWND) {
    // SAFETY: hwnd is a live window of this thread.
    unsafe { ShowWindow(hwnd, SW_HIDE) };
}

fn set_alpha(hwnd: HWND, alpha: u8) {
    // SAFETY: hwnd is a live layered window of this thread.
    unsafe { SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) };
}

/// Clips the window to a ring so the landing point itself stays visible.
fn set_ring_shape(hwnd: HWND, diameter: i32, ring: i32) {
    // SAFETY: Regions are created, combined and released here; on success the
    // window takes ownership of `ring_region`, otherwise it is deleted.
    unsafe {
        let ring_region = CreateEllipticRgn(0, 0, diameter, diameter);
        let hole = CreateEllipticRgn(ring, ring, diameter - ring, diameter - ring);
        CombineRgn(ring_region, ring_region, hole, RGN_DIFF);
        DeleteObject(hole);
        if SetWindowRgn(hwnd, ring_region, 0) == 0 {
            DeleteObject(ring_region);
        }
    }
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
