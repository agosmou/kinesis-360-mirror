//! Headless capture probe: prints raw HID usages from the Kinesis with no GUI.
//!
//! Useful for checking permissions and confirming the board is being seen
//! before blaming the frontend.
//!
//!     cargo run --example probe

use std::sync::mpsc;
use std::time::Duration;

use kinesis_360_mirror_lib::hid;

fn main() {
    println!("access: {:?}", hid::check_access());

    if hid::check_access() != hid::Access::Granted {
        println!("requesting Input Monitoring (approve the system prompt)…");
        println!("granted: {}", hid::request_access());
        println!("access now: {:?}", hid::check_access());
    }

    let (tx, rx) = mpsc::channel();
    match hid::spawn(move |event| {
        let _ = tx.send(event);
    }) {
        Ok(()) => println!("capture started — type on the Kinesis (10s)…"),
        Err(err) => {
            eprintln!("capture failed: {err}");
            std::process::exit(1);
        }
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut seen = 0usize;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(ev) => {
                seen += 1;
                println!(
                    "  usage 0x{:02X} {}",
                    ev.usage,
                    if ev.down { "down" } else { "up" }
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    println!("done — {seen} events");
}
