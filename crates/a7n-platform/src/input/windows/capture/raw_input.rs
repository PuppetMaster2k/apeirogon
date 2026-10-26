// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::mem::size_of;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RAWMOUSE,
    RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassW, UnregisterClassW,
    WM_INPUT, WNDCLASSW,
};

use super::STATE;

const CLASS_NAME: *const u16 = windows_sys::core::w!("a7n-raw-input");
const GENERIC_DESKTOP_PAGE: u16 = 0x01;
const MOUSE_USAGE: u16 = 0x02;

pub(super) struct Window {
    handle: HWND,
    module: HINSTANCE,
    registered: bool,
}

impl Window {
    pub(super) fn install(module: HINSTANCE) -> io::Result<Self> {
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        // SAFETY: class and its null-terminated name remain valid during the call;
        // the callback lives until the class is unregistered on this thread.
        if unsafe { RegisterClassW(&raw const class) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut window = Self {
            handle: null_mut(),
            module,
            registered: false,
        };
        // SAFETY: The class is registered and HWND_MESSAGE creates a window owned
        // by this capture thread, which pumps its messages until Drop.
        window.handle = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME,
                null(),
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
        if window.handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        register(window.handle, RIDEV_INPUTSINK)?;
        window.registered = true;
        Ok(window)
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if self.registered {
            let _ = register(null_mut(), RIDEV_REMOVE);
        }
        // SAFETY: This thread owns the window and class, including on partial
        // startup failure; destroy the window before unregistering its class.
        unsafe {
            if !self.handle.is_null() {
                DestroyWindow(self.handle);
            }
            UnregisterClassW(CLASS_NAME, self.module);
        }
    }
}

fn register(window: HWND, flags: u32) -> io::Result<()> {
    let device = RAWINPUTDEVICE {
        usUsagePage: GENERIC_DESKTOP_PAGE,
        usUsage: MOUSE_USAGE,
        dwFlags: flags,
        hwndTarget: window,
    };
    let size = u32::try_from(size_of::<RAWINPUTDEVICE>()).map_err(io::Error::other)?;
    // SAFETY: device describes exactly one initialized record; removal uses a
    // null window as required by RIDEV_REMOVE.
    if unsafe { RegisterRawInputDevices(&raw const device, 1, size) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_mouse(lparam: LPARAM) {
    let mut input = RAWINPUT::default();
    let Ok(capacity) = u32::try_from(size_of::<RAWINPUT>()) else {
        return;
    };
    let Ok(header_size) = u32::try_from(size_of::<RAWINPUTHEADER>()) else {
        return;
    };
    let mut size = capacity;
    // SAFETY: WM_INPUT supplies this handle for the callback's lifetime. input
    // is aligned, writable storage and size bounds the write to that storage.
    let read = unsafe {
        GetRawInputData(
            lparam as _,
            RID_INPUT,
            (&raw mut input).cast(),
            &raw mut size,
            header_size,
        )
    };
    if read == u32::MAX
        || read > capacity
        || usize::try_from(read).unwrap_or_default()
            < size_of::<RAWINPUTHEADER>() + size_of::<RAWMOUSE>()
        || input.header.dwSize != read
        || input.header.dwType != RIM_TYPEMOUSE
    {
        return;
    }
    // SAFETY: The validated type is RIM_TYPEMOUSE and the full mouse record was read.
    let mouse = unsafe { input.data.mouse };
    STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.raw_motion(
                !input.header.hDevice.is_null(),
                mouse.usFlags & MOUSE_MOVE_ABSOLUTE != 0,
                mouse.lLastX,
                mouse.lLastY,
            );
        }
    });
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        read_mouse(lparam);
    }
    // SAFETY: Forward original window parameters, including WM_INPUT so Windows
    // can perform foreground raw-input cleanup. No TLS borrow remains held.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
