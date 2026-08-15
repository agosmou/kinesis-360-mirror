//! Passive HID capture for the Kinesis Advantage 360 Pro.
//!
//! Both platforms report the *raw HID usage* the keyboard sent, not whatever
//! the OS layout turned it into. That is what we want: the board resolves its
//! own layers and remaps onboard, so the usage is the closest thing the host
//! can see to "which physical key moved".

use serde::Serialize;

/// The board enumerates under two different IDs depending on transport, and
/// can be connected over both at once. Watch both or the mirror dies on unplug.
pub const KINESIS_IDS: &[(u16, u16)] = &[
    (0x29EA, 0x0362), // Kinesis Corporation, "Adv360 Pro" over USB
    (0x1D50, 0x615E), // OpenMoko/ZMK shared VID, over Bluetooth LE
];

/// HID usage page 0x07 = Keyboard/Keypad. We ignore everything else
/// (consumer controls, LEDs, system power) to keep the board quiet.
pub const USAGE_PAGE_KEYBOARD: u32 = 0x07;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyEvent {
    /// HID usage ID within the keyboard page, e.g. 0x04 = A.
    pub usage: u32,
    pub down: bool,
}

/// Whether we are allowed to observe input at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Granted,
    Denied,
    /// Never asked yet (macOS), or device nodes unreadable (Linux).
    Unknown,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{check_access, request_access, spawn};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{check_access, request_access, spawn};

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub use unsupported::{check_access, request_access, spawn};
