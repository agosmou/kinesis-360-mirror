//! Linux capture via evdev.
//!
//! Reading `/dev/input/event*` sits below the compositor, so this works
//! identically on X11 and Wayland — Wayland deliberately offers no global
//! key-listening protocol, which rules out the usual portal approaches.
//!
//! Needs read access to the device nodes. Either add yourself to the `input`
//! group, or drop a udev rule granting uaccess for 29ea:0362 (see README).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use evdev::{Device, InputEventKind};

use super::{Access, KeyEvent, KINESIS_IDS};

/// Linux input-event-codes -> HID keyboard usage page (0x07) IDs.
///
/// evdev normalizes to its own keycode space, so we translate back to raw HID
/// to give the frontend one event shape across both platforms.
const LINUX_TO_HID: &[(u16, u32)] = &[
    (1, 0x29),   // ESC
    (2, 0x1E), (3, 0x1F), (4, 0x20), (5, 0x21), (6, 0x22),
    (7, 0x23), (8, 0x24), (9, 0x25), (10, 0x26), (11, 0x27), // 1..0
    (12, 0x2D), (13, 0x2E), (14, 0x2A), (15, 0x2B),          // - = BKSP TAB
    (16, 0x14), (17, 0x1A), (18, 0x08), (19, 0x15), (20, 0x17),
    (21, 0x1C), (22, 0x18), (23, 0x0C), (24, 0x12), (25, 0x13), // Q..P
    (26, 0x2F), (27, 0x30), (28, 0x28), (29, 0xE0),          // [ ] ENTER LCTRL
    (30, 0x04), (31, 0x16), (32, 0x07), (33, 0x09), (34, 0x0A),
    (35, 0x0B), (36, 0x0D), (37, 0x0E), (38, 0x0F),          // A..L
    (39, 0x33), (40, 0x34), (41, 0x35), (42, 0xE1), (43, 0x31), // ; ' ` LSHFT \
    (44, 0x1D), (45, 0x1B), (46, 0x06), (47, 0x19), (48, 0x05),
    (49, 0x11), (50, 0x10),                                   // Z..M
    (51, 0x36), (52, 0x37), (53, 0x38), (54, 0xE5),          // , . / RSHFT
    (55, 0x55), (56, 0xE2), (57, 0x2C), (58, 0x39),          // KP* LALT SPACE CAPS
    (59, 0x3A), (60, 0x3B), (61, 0x3C), (62, 0x3D), (63, 0x3E),
    (64, 0x3F), (65, 0x40), (66, 0x41), (67, 0x42), (68, 0x43), // F1..F10
    (69, 0x53), (70, 0x47),                                   // NUMLOCK SCROLLLOCK
    (71, 0x5F), (72, 0x60), (73, 0x61), (74, 0x56),          // KP7-9 KP-
    (75, 0x5C), (76, 0x5D), (77, 0x5E), (78, 0x57),          // KP4-6 KP+
    (79, 0x59), (80, 0x5A), (81, 0x5B), (82, 0x62), (83, 0x63), // KP1-3 KP0 KP.
    (87, 0x44), (88, 0x45),                                   // F11 F12
    (96, 0x58), (97, 0xE4), (98, 0x54), (99, 0x46), (100, 0xE6), // KPENTER RCTRL KP/ SYSRQ RALT
    (102, 0x4A), (103, 0x52), (104, 0x4B), (105, 0x50),      // HOME UP PGUP LEFT
    (106, 0x4F), (107, 0x4D), (108, 0x51), (109, 0x4E),      // RIGHT END DOWN PGDN
    (110, 0x49), (111, 0x4C), (117, 0x67), (119, 0x48),      // INS DEL KP= PAUSE
    (125, 0xE3), (126, 0xE7),                                 // LMETA RMETA
];

fn to_hid(code: u16) -> Option<u32> {
    LINUX_TO_HID
        .iter()
        .find(|(linux, _)| *linux == code)
        .map(|(_, hid)| *hid)
}

fn is_kinesis(dev: &Device) -> bool {
    let id = dev.input_id();
    KINESIS_IDS
        .iter()
        .any(|(vid, pid)| id.vendor() == *vid && id.product() == *pid)
}

/// We can't know about a device we can't open, so "can we see any input node"
/// stands in for the permission check.
pub fn check_access() -> Access {
    match std::fs::read_dir("/dev/input") {
        Ok(entries) => {
            let mut saw_node = false;
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.to_string_lossy().contains("event") {
                    continue;
                }
                saw_node = true;
                if Device::open(&path).is_ok() {
                    return Access::Granted;
                }
            }
            if saw_node {
                Access::Denied
            } else {
                Access::Unknown
            }
        }
        Err(_) => Access::Denied,
    }
}

/// Nothing to prompt for on Linux — access is a udev/group matter the user
/// has to fix out of band.
pub fn request_access() -> bool {
    check_access() == Access::Granted
}

pub fn spawn<F>(sink: F) -> Result<(), String>
where
    F: Fn(KeyEvent) + Send + Sync + 'static,
{
    let sink = Arc::new(sink);
    let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));

    // Supervisor: rescan for the board so unplug/replug and USB<->BLE
    // handoff recover on their own.
    std::thread::Builder::new()
        .name("kinesis-hid-scan".into())
        .spawn(move || loop {
            for (path, device) in evdev::enumerate() {
                if !is_kinesis(&device) {
                    continue;
                }
                {
                    let mut guard = open.lock().unwrap();
                    if !guard.insert(path.clone()) {
                        continue; // already reading this node
                    }
                }
                let sink = Arc::clone(&sink);
                let open = Arc::clone(&open);
                let reader_path = path.clone();
                std::thread::spawn(move || {
                    read_device(device, sink.as_ref());
                    open.lock().unwrap().remove(&reader_path);
                });
            }
            std::thread::sleep(Duration::from_secs(2));
        })
        .map_err(|e| format!("failed to spawn scan thread: {e}"))?;

    Ok(())
}

fn read_device<F>(mut device: Device, sink: &F)
where
    F: Fn(KeyEvent) + Send + Sync + 'static,
{
    loop {
        let events = match device.fetch_events() {
            Ok(events) => events,
            Err(_) => return, // unplugged; supervisor will pick it back up
        };
        for event in events {
            let InputEventKind::Key(key) = event.kind() else {
                continue;
            };
            // 0 = release, 1 = press, 2 = autorepeat (ignored: not a new press)
            let down = match event.value() {
                0 => false,
                1 => true,
                _ => continue,
            };
            if let Some(usage) = to_hid(key.code()) {
                sink(KeyEvent { usage, down });
            }
        }
    }
}
