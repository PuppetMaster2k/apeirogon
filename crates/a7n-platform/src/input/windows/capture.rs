// SPDX-License-Identifier: GPL-3.0-or-later

//! Low-level hooks run only on their installing thread, alongside the message loop.

use std::cell::RefCell;
use std::io;
use std::ptr::null_mut;
use std::sync::mpsc as sync;
use std::thread;
use std::time::Instant;

use a7n_proto::v1::{InputEvent, KeyEvent, input_event};
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::mpsc::{Receiver, Sender};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_NUMLOCK, VK_PAUSE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, KillTimer,
    LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_UP, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SetTimer,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_MOUSEMOVE,
};

use super::{INJECTION_MARKER, keymap, mouse, pointer_position};
use crate::input::{CaptureControl, Captured, DeadMan, InputError};

const WAKE_INTERVAL_MS: u32 = 50;
const SCAN_CODE_COUNT: usize = 256;
const HELD_KEY_COUNT: usize = SCAN_CODE_COUNT * 3;

thread_local! {
    static STATE: RefCell<Option<CaptureState>> = const { RefCell::new(None) };
}

struct CaptureState {
    events: Sender<Captured>,
    deadman: DeadMan,
    grabbed: bool,
    queue_full: bool,
    last: POINT,
    keys: [bool; HELD_KEY_COUNT],
}

pub(super) fn spawn(
    events: Sender<Captured>,
    control: Receiver<CaptureControl>,
) -> Result<(), InputError> {
    let (ready_tx, ready_rx) = sync::channel();
    thread::Builder::new()
        .name("a7n-windows-capture".to_owned())
        .spawn(move || {
            let setup = pointer_position().and_then(|last| {
                STATE.with(|state| {
                    *state.borrow_mut() = Some(CaptureState {
                        events,
                        deadman: DeadMan::new(Instant::now()),
                        grabbed: false,
                        queue_full: false,
                        last,
                        keys: [false; HELD_KEY_COUNT],
                    });
                });
                Hooks::install()
            });
            match setup {
                Ok(hooks) => {
                    if ready_tx.send(Ok(())).is_ok() {
                        run(control);
                    }
                    drop(hooks);
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
            STATE.with(|state| state.borrow_mut().take());
        })
        .map_err(InputError::Thread)?;
    ready_rx
        .recv()
        .map_err(|error| io::Error::other(error.to_string()))??;
    Ok(())
}

struct Hooks {
    keyboard: HHOOK,
    mouse: HHOOK,
    timer: usize,
}

impl Hooks {
    fn install() -> io::Result<Self> {
        let mut hooks = Self {
            keyboard: null_mut(),
            mouse: null_mut(),
            timer: 0,
        };
        // SAFETY: Null requests the current executable's module; it stays loaded.
        let module = unsafe { GetModuleHandleW(std::ptr::null()) };
        if module.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: Callbacks have the required ABI and remain valid for the hooks'
        // lifetime; this thread pumps messages until Drop removes both hooks.
        unsafe {
            hooks.keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
            if hooks.keyboard.is_null() {
                return Err(io::Error::last_os_error());
            }
            hooks.mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
            if hooks.mouse.is_null() {
                return Err(io::Error::last_os_error());
            }
            hooks.timer = SetTimer(null_mut(), 0, WAKE_INTERVAL_MS, None);
        }
        if hooks.timer == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(hooks)
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        // SAFETY: These handles belong to this thread and are removed once,
        // including partial installation failure. Null/zero handles are skipped.
        unsafe {
            if self.timer != 0 {
                KillTimer(null_mut(), self.timer);
            }
            if !self.mouse.is_null() {
                UnhookWindowsHookEx(self.mouse);
            }
            if !self.keyboard.is_null() {
                UnhookWindowsHookEx(self.keyboard);
            }
        }
    }
}

fn run(mut control: Receiver<CaptureControl>) {
    let mut message = MSG::default();
    loop {
        if !STATE.with(|state| {
            state
                .borrow_mut()
                .as_mut()
                .is_some_and(|s| s.tick(&mut control, Instant::now()))
        }) {
            return;
        }
        // SAFETY: message is writable MSG storage. A null window pumps this
        // thread's queue, including the timer and low-level hook dispatch.
        let result = unsafe { GetMessageW(&raw mut message, null_mut(), 0, 0) };
        if result <= 0 {
            return;
        }
        // SAFETY: message was populated by GetMessageW; no TLS borrow is held
        // while Windows may call one of our hooks.
        unsafe { DispatchMessageW(&raw const message) };
    }
}

impl CaptureState {
    fn tick(&mut self, control: &mut Receiver<CaptureControl>, now: Instant) -> bool {
        loop {
            match control.try_recv() {
                Ok(CaptureControl::Keepalive) => self.deadman.keepalive(now),
                Ok(CaptureControl::Grab(grab)) => {
                    if grab {
                        self.deadman.keepalive(now);
                    }
                    self.grabbed = false;
                    match pointer_position() {
                        Ok(point) => {
                            self.last = point;
                            self.grabbed = grab;
                        }
                        Err(_) => eprintln!("kind=input_grab_failed"),
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        if self.grabbed && self.deadman.expired(now) {
            self.grabbed = false;
            eprintln!("kind=input_deadman_ungrab");
        }
        // Logging stays outside callbacks, which must never wait for stderr.
        if std::mem::take(&mut self.queue_full) {
            eprintln!("kind=capture_queue_full");
        }
        !self.events.is_closed()
    }

    fn emit(&mut self, event: Captured) {
        match self.events.try_send(event) {
            Err(TrySendError::Full(_)) => self.queue_full = true,
            Err(TrySendError::Closed(_)) => self.grabbed = false,
            Ok(()) => {}
        }
    }

    fn key(&mut self, key: &KBDLLHOOKSTRUCT) -> bool {
        if own_injection(key.flags & LLKHF_INJECTED != 0, key.dwExtraInfo) {
            return false;
        }
        let usage = keymap::hid_from_media(key.vkCode).or_else(|| {
            // Pause uses E1, which LLKHF_EXTENDED cannot represent. Num Lock is
            // the E0 key with the same low scan byte on Windows.
            let extended = key.vkCode == u32::from(VK_NUMLOCK)
                || (key.vkCode != u32::from(VK_PAUSE) && key.flags & LLKHF_EXTENDED != 0);
            keymap::hid_from_scan(key.scanCode, extended)
        });
        // Scan identity keeps keypad/navigation and left/right modifiers distinct,
        // even when Windows assigns them the same virtual key.
        let index = if keymap::hid_from_media(key.vkCode).is_some() {
            usize::try_from(key.vkCode)
                .ok()
                .map(|vk| SCAN_CODE_COUNT * 2 + vk)
        } else {
            usize::try_from(key.scanCode)
                .ok()
                .filter(|&scan| scan < SCAN_CODE_COUNT)
                .map(|scan| {
                    scan + if key.flags & LLKHF_EXTENDED != 0 {
                        SCAN_CODE_COUNT
                    } else {
                        0
                    }
                })
        };
        if let (Some(usage), Some(held)) = (usage, index.and_then(|index| self.keys.get_mut(index)))
        {
            let down = key.flags & LLKHF_UP == 0;
            let repeat = down && *held;
            *held = down;
            if !repeat {
                self.emit(Captured::Input(InputEvent {
                    event: Some(input_event::Event::Key(KeyEvent {
                        usage,
                        down,
                        character: String::new(),
                    })),
                }));
            }
        }
        self.grabbed
    }

    fn mouse(&mut self, message: u32, mouse: &MSLLHOOKSTRUCT) -> bool {
        if own_injection(mouse.flags & LLMHF_INJECTED != 0, mouse.dwExtraInfo) {
            if message == WM_MOUSEMOVE {
                self.last = mouse.pt;
            }
            return false;
        }
        if message == WM_MOUSEMOVE {
            // shortcut: low-level hooks expose accelerated deltas; use Raw Input
            // if users need unaccelerated motion.
            let event = self.motion(mouse.pt);
            self.emit(event);
        } else if let Some(event) = mouse::translate(message, mouse.mouseData) {
            self.emit(Captured::Input(InputEvent { event: Some(event) }));
        }
        self.grabbed
    }

    fn motion(&mut self, point: POINT) -> Captured {
        let dx = point.x.saturating_sub(self.last.x);
        let dy = point.y.saturating_sub(self.last.y);
        if !self.grabbed {
            self.last = point;
        }
        Captured::Pointer {
            x: self.last.x,
            y: self.last.y,
            dx,
            dy,
        }
    }
}

fn own_injection(injected: bool, marker: usize) -> bool {
    injected && marker == INJECTION_MARKER
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: For a nonnegative keyboard hook code, Windows supplies a
        // KBDLLHOOKSTRUCT valid for the duration of this callback.
        let key = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if STATE.with(|s| s.borrow_mut().as_mut().is_some_and(|s| s.key(key))) {
            return 1;
        }
    }
    // SAFETY: Forward the untouched callback parameters; this API ignores hhk.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: For a nonnegative mouse hook code, Windows supplies an
        // MSLLHOOKSTRUCT valid for the duration of this callback.
        let mouse = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if let Ok(message) = u32::try_from(wparam) {
            if STATE.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .is_some_and(|s| s.mouse(message, mouse))
            }) {
                return 1;
            }
        }
    }
    // SAFETY: Forward the untouched callback parameters; this API ignores hhk.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests;
