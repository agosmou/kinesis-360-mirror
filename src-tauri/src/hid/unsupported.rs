//! Fallback so the crate still builds on platforms we have not wired up.

use super::{Access, KeyEvent};

pub fn check_access() -> Access {
    Access::Denied
}

pub fn request_access() -> bool {
    false
}

pub fn spawn<F>(_sink: F) -> Result<(), String>
where
    F: Fn(KeyEvent) + Send + Sync + 'static,
{
    Err("HID capture is only implemented for macOS and Linux".into())
}
