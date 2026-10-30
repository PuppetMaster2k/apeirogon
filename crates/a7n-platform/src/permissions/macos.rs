// SPDX-License-Identifier: GPL-3.0-or-later

use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};
use objc2_core_graphics::{CGPreflightListenEventAccess, CGRequestListenEventAccess};
use objc2_foundation::{NSString, NSURL};

use super::{Permission, PermissionError};

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const INPUT_MONITORING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    static kAXTrustedCheckOptionPrompt: &'static CFString;
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: Option<&CFDictionary>) -> bool;
}

pub(super) fn missing() -> Vec<Permission> {
    let mut missing = Vec::new();
    // SAFETY: This read-only permission query has no arguments or preconditions.
    if !unsafe { AXIsProcessTrusted() } {
        missing.push(Permission::Accessibility);
    }
    if !CGPreflightListenEventAccess() {
        missing.push(Permission::InputMonitoring);
    }
    missing
}

pub(super) fn request(permission: Permission) -> Result<(), PermissionError> {
    match permission {
        Permission::Accessibility => prompt_accessibility(),
        Permission::InputMonitoring => {
            CGRequestListenEventAccess();
        }
    }
    open_pane(permission)
}

/// Passing the prompt option also lists the daemon under Accessibility.
fn prompt_accessibility() {
    // SAFETY: The static is an immutable CFString constant owned by the
    // ApplicationServices framework and valid for the life of the process.
    let key = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: `options` is a valid CFDictionary that outlives the call; the
    // function only reads it.
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
}

fn open_pane(permission: Permission) -> Result<(), PermissionError> {
    let link = match permission {
        Permission::Accessibility => ACCESSIBILITY_PANE,
        Permission::InputMonitoring => INPUT_MONITORING_PANE,
    };
    let url = NSURL::URLWithString(&NSString::from_str(link))
        .ok_or(PermissionError::OpenSettings(permission))?;
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(PermissionError::OpenSettings(permission))
    }
}
