// SPDX-License-Identifier: GPL-3.0-or-later

//! Gives macOS code a main thread that runs a run loop.
//!
//! `AppKit` raises an exception when a window is created or shown off the
//! process main thread, and a plain tokio `main` blocks that thread in
//! `block_on`. [`run`] moves the program body to a worker thread and keeps the
//! real main thread in its `CFRunLoop`, where backends such as
//! [`crate::feedback`] schedule their window work. Elsewhere it just calls the
//! body.
//!
//! Call it first thing in `main`, before building the tokio runtime.

/// Runs `main` and returns its result; see the module documentation.
///
/// # Panics
///
/// On macOS when not called from the process main thread. A panic inside
/// `main` is resumed on the calling thread.
pub fn run<R: Send + 'static>(main: impl FnOnce() -> R + Send + 'static) -> R {
    #[cfg(target_os = "macos")]
    {
        macos::run(main)
    }
    #[cfg(not(target_os = "macos"))]
    {
        main()
    }
}

#[cfg(target_os = "macos")]
pub(crate) use macos::{Waker, active};

#[cfg(target_os = "macos")]
mod macos {
    use std::any::Any;
    use std::ffi::c_void;
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_core_foundation::{
        CFRetained, CFRunLoop, CFRunLoopSource, CFRunLoopSourceContext, kCFRunLoopDefaultMode,
    };

    /// Set while [`run`] keeps the main thread in its run loop.
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    /// Set by the worker thread once the program body has returned.
    static FINISHED: AtomicBool = AtomicBool::new(false);

    /// Whether the main run loop is being kept alive by [`run`].
    pub(crate) fn active() -> bool {
        ACTIVE.load(Ordering::Acquire)
    }

    /// A run-loop source other threads can signal to run `perform` on the
    /// loop's thread.
    pub(crate) struct Waker {
        source: CFRetained<CFRunLoopSource>,
        run_loop: CFRetained<CFRunLoop>,
    }

    // SAFETY: CFRunLoopSourceSignal and CFRunLoopWakeUp are documented as safe
    // to call from any thread; the Waker exposes nothing else.
    unsafe impl Send for Waker {}

    impl Waker {
        /// Adds a source to `run_loop` that calls `perform(info)` on its thread.
        ///
        /// # Safety
        ///
        /// `info` must stay valid for as long as the source can fire, and
        /// `perform` must be sound to call with it.
        pub(crate) unsafe fn new(
            run_loop: CFRetained<CFRunLoop>,
            perform: unsafe extern "C-unwind" fn(*mut c_void),
            info: *mut c_void,
        ) -> Option<Self> {
            let mut context = CFRunLoopSourceContext {
                version: 0,
                info,
                retain: None,
                release: None,
                copyDescription: None,
                equal: None,
                hash: None,
                schedule: None,
                cancel: None,
                perform: Some(perform),
            };
            // SAFETY: The default allocator accepts None and the context is
            // valid for the call; the caller vouches for `info` and `perform`.
            let source = unsafe { CFRunLoopSource::new(None, 0, &raw mut context) }?;
            // SAFETY: Core Foundation's default run-loop mode is an immutable global.
            run_loop.add_source(Some(&source), unsafe { kCFRunLoopDefaultMode });
            Some(Self { source, run_loop })
        }

        pub(crate) fn wake(&self) {
            self.source.signal();
            self.run_loop.wake_up();
        }
    }

    pub(super) fn run<R: Send + 'static>(main: impl FnOnce() -> R + Send + 'static) -> R {
        let mtm = MainThreadMarker::new().expect("run is called on the main thread");
        // Without this a plain process would get a Dock icon and Cmd-Tab entry.
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let run_loop = CFRunLoop::main().expect("the main thread has a run loop");
        // SAFETY: `stop_run_loop` ignores its info pointer, which is null.
        let waker = unsafe { Waker::new(run_loop, stop_run_loop, std::ptr::null_mut()) }
            .expect("a run-loop source can be created");
        ACTIVE.store(true, Ordering::Release);
        let worker = thread::Builder::new()
            .name("a7n-main".into())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(main));
                FINISHED.store(true, Ordering::Release);
                waker.wake();
                result
            })
            .expect("the program thread can be spawned");
        // The signalled source stays pending, so a wake before the loop starts
        // is not lost; the flag covers a loop that returns for other reasons.
        while !FINISHED.load(Ordering::Acquire) {
            CFRunLoop::run();
        }
        ACTIVE.store(false, Ordering::Release);
        match worker
            .join()
            .expect("the program thread reports its result")
        {
            Ok(result) => result,
            Err(payload) => resume(payload),
        }
    }

    fn resume(payload: Box<dyn Any + Send>) -> ! {
        panic::resume_unwind(payload)
    }

    unsafe extern "C-unwind" fn stop_run_loop(_: *mut c_void) {
        if let Some(run_loop) = CFRunLoop::current() {
            run_loop.stop();
        }
    }
}
