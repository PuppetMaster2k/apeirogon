// SPDX-License-Identifier: GPL-3.0-or-later

//! A message-only window owns clipboard access and delayed rendering.

mod data;
mod image;
mod receive;
mod serve;

use std::cell::RefCell;
use std::io;
use std::ptr::{null, null_mut};
use std::sync::mpsc as sync;
use std::thread;

use a7n_core::clipboard::{Generation, LocalChange};
use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, HWND_MESSAGE,
    MSG, PostMessageW, RegisterClassW, WM_APP, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROYCLIPBOARD,
    WM_RENDERALLFORMATS, WM_RENDERFORMAT, WNDCLASSW,
};
use windows_sys::core::w;

use super::{Clipboard, ClipboardCommand, LazyItem, PasteRequest};

const CHANNEL_CAPACITY: usize = 16;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

struct State {
    formats: data::Formats,
    changes: mpsc::Sender<LocalChange>,
    pastes: mpsc::Sender<PasteRequest>,
    generation: Option<Generation>,
    lazy: Vec<(u32, LazyItem)>,
    sequence: Option<u32>,
}

pub(super) fn start() -> io::Result<Clipboard> {
    let (changes_tx, changes) = mpsc::channel(CHANNEL_CAPACITY);
    let (commands, mut commands_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (pastes_tx, pastes) = mpsc::channel(CHANNEL_CAPACITY);
    let (inbox_tx, inbox) = mpsc::channel(CHANNEL_CAPACITY);
    let (ready_tx, ready_rx) = sync::channel();
    thread::Builder::new()
        .name("a7n-windows-clipboard".into())
        .spawn(move || {
            let setup = data::Formats::new().and_then(|formats| {
                let hwnd = create_window()?;
                STATE.with(|state| {
                    *state.borrow_mut() = Some(State {
                        formats,
                        changes: changes_tx,
                        pastes: pastes_tx,
                        generation: None,
                        lazy: Vec::new(),
                        sequence: None,
                    });
                });
                Ok(hwnd)
            });
            match setup {
                Ok(hwnd) => {
                    if ready_tx.send(Ok(hwnd as usize)).is_ok() {
                        run(hwnd, inbox);
                    }
                    // SAFETY: hwnd was created on this thread and is destroyed once.
                    unsafe {
                        RemoveClipboardFormatListener(hwnd);
                        DestroyWindow(hwnd);
                    }
                    STATE.with(|state| state.borrow_mut().take());
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
        })?;
    let window = ready_rx.recv().map_err(io::Error::other)??;
    let relay = thread::Builder::new()
        .name("a7n-clipboard-commands".into())
        .spawn(move || {
            while let Some(command) = commands_rx.blocking_recv() {
                if inbox_tx.blocking_send(command).is_err() {
                    return;
                }
                // SAFETY: Posts a pointer-free wake-up to the clipboard thread.
                if unsafe { PostMessageW(window as HWND, WM_APP, 0, 0) } == 0 {
                    return;
                }
            }
            // SAFETY: Posts a pointer-free shutdown to the clipboard thread.
            unsafe { PostMessageW(window as HWND, WM_CLOSE, 0, 0) };
        });
    if let Err(error) = relay {
        // SAFETY: Startup failed; ask the owning thread to destroy its window.
        unsafe { PostMessageW(window as HWND, WM_CLOSE, 0, 0) };
        return Err(error);
    }
    Ok(Clipboard {
        images: true,
        changes,
        commands,
        pastes,
    })
}

fn create_window() -> io::Result<HWND> {
    // SAFETY: Requests the current module, which outlives the window class.
    let module = unsafe { GetModuleHandleW(null()) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: module,
        lpszClassName: w!("ApeirogonClipboard"),
        ..WNDCLASSW::default()
    };
    // SAFETY: class and its static name/callback are valid; registration is
    // process-wide and can already exist if this backend was started before.
    unsafe { RegisterClassW(&raw const class) };
    // SAFETY: Registered class has a static callback; no borrowed creation data.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.lpszClassName,
            w!(""),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            module,
            null(),
        )
    };
    if hwnd.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: hwnd is a live window on this thread.
    if unsafe { AddClipboardFormatListener(hwnd) } == 0 {
        let error = io::Error::last_os_error();
        // SAFETY: Destroy the window on its creating thread after setup failure.
        unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    Ok(hwnd)
}

fn run(hwnd: HWND, mut inbox: mpsc::Receiver<ClipboardCommand>) {
    with_state(|state| state.changed(hwnd));
    let mut message = MSG::default();
    // SAFETY: message is writable storage; this thread owns the message queue.
    while unsafe { GetMessageW(&raw mut message, null_mut(), 0, 0) } > 0 {
        if message.message == WM_CLOSE && message.hwnd == hwnd {
            break;
        }
        if message.message == WM_APP && message.hwnd == hwnd {
            while let Ok(command) = inbox.try_recv() {
                with_state(|state| state.set(hwnd, command));
            }
        } else {
            // SAFETY: message was initialized by GetMessageW; no state borrow is held.
            unsafe { DispatchMessageW(&raw const message) };
        }
    }
}

fn with_state(action: impl FnOnce(&mut State)) {
    // Windows can synchronously reenter the callback (notably EmptyClipboard).
    // Temporarily remove state so nested notifications cannot alias or borrow it.
    let state = STATE.with(|slot| slot.borrow_mut().take());
    if let Some(mut state) = state {
        action(&mut state);
        STATE.with(|slot| *slot.borrow_mut() = Some(state));
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CLIPBOARDUPDATE => with_state(|state| state.changed(hwnd)),
        WM_DESTROYCLIPBOARD => with_state(|state| {
            state.generation = None;
            state.lazy.clear();
        }),
        WM_RENDERFORMAT => {
            if let Ok(format) = u32::try_from(wparam) {
                with_state(|state| state.render(format));
            }
        }
        WM_RENDERALLFORMATS => with_state(|state| state.render_all(hwnd)),
        _ => {
            // SAFETY: Forward unchanged window procedure parameters to Windows.
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
    }
    0
}
