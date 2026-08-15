pub mod hid;

use std::collections::HashSet;
use std::sync::Mutex;

use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

/// Tracks which usages are currently held so we can drop duplicate presses.
/// The board can be attached over USB and BLE at the same time; ZMK normally
/// sends to one endpoint, but a transport handoff can briefly double-report.
#[derive(Default)]
struct Held(Mutex<HashSet<u32>>);

#[tauri::command]
fn hid_access() -> hid::Access {
    hid::check_access()
}

#[tauri::command]
fn hid_request_access() -> bool {
    hid::request_access()
}

/// Click-through: the mirror should never eat a click meant for the editor
/// underneath. Turned off briefly when the user wants to drag or configure it.
#[tauri::command]
fn set_click_through(window: tauri::Window, enabled: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(enabled)
        .map_err(|e| e.to_string())
}

/// Backs the in-window "on top" pill. Reading the state is covered by
/// core:window:default, but setting it is not, so it goes through here.
#[tauri::command]
fn set_always_on_top(window: tauri::Window, enabled: bool) -> Result<(), String> {
    window.set_always_on_top(enabled).map_err(|e| e.to_string())
}

/// Resize from the frontend, which is the only side that knows the board's
/// aspect ratio. core:window:default can read the size but not set it.
#[tauri::command]
fn resize_window(window: tauri::Window, width: f64, height: f64) -> Result<(), String> {
    // The frontend clamps too, but validate here as well: a command is part of
    // the app's IPC surface, and NaN or an absurd size should not reach AppKit
    // just because the page asked nicely.
    if !width.is_finite() || !height.is_finite() {
        return Err("width and height must be finite".into());
    }
    window
        .set_size(tauri::LogicalSize::new(
            width.clamp(200.0, 10_000.0),
            height.clamp(150.0, 10_000.0),
        ))
        .map_err(|e| e.to_string())
}

/// Frontend diagnostics on stderr. The webview console is not visible when
/// the app runs as a bundle, and a mirror that fails silently is useless.
#[tauri::command]
fn ui_log(message: String) {
    // Frontend-controlled text landing in someone's terminal: strip control
    // characters so it cannot inject ANSI escapes, and cap the length.
    let safe: String = message
        .chars()
        .filter(|c| !c.is_control())
        .take(500)
        .collect();
    eprintln!("[ui] {safe}");
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    // The red button hides rather than quits, so this is the way back to a
    // hidden window — and the only way to actually quit.
    let visible = CheckMenuItemBuilder::with_id("visible", "Show window")
        .checked(true)
        .build(app)?;
    let on_top = CheckMenuItemBuilder::with_id("on_top", "Always on top")
        .checked(true)
        .build(app)?;
    let click_through = CheckMenuItemBuilder::with_id("click_through", "Click-through")
        .checked(false)
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Kinesis 360 Mirror").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&visible, &on_top, &click_through, &quit])
        .build()?;

    let mut builder = TrayIconBuilder::with_id("mirror").menu(&menu);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "visible" => {
                let Some(window) = app.get_webview_window("main") else {
                    return;
                };
                if visible.is_checked().unwrap_or(true) {
                    let _ = window.show();
                } else {
                    let _ = window.hide();
                }
            }
            "on_top" => {
                let Some(window) = app.get_webview_window("main") else {
                    return;
                };
                let _ = window.set_always_on_top(on_top.is_checked().unwrap_or(true));
            }
            "click_through" => {
                let Some(window) = app.get_webview_window("main") else {
                    return;
                };
                // The check item has already flipped by the time we see the
                // event, so its state is the value we want to apply.
                let enabled = click_through.is_checked().unwrap_or(true);
                let _ = window.set_ignore_cursor_events(enabled);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Held::default())
        .invoke_handler(tauri::generate_handler![
            hid_access,
            hid_request_access,
            set_click_through,
            set_always_on_top,
            resize_window,
            ui_log
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            eprintln!(
                "[mirror] kinesis-360-mirror {} starting; input access: {:?}",
                app.package_info().version,
                hid::check_access()
            );

            // Start interactive, NOT click-through: on first run the window
            // has to be positioned and the permission gate has to be
            // clickable. Click-through is opt-in from the tray once the mirror
            // is where the user wants it.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_ignore_cursor_events(false);

                // The title bar's red button would otherwise destroy the only
                // window and leave the app running headless. Hide instead, so
                // the tray's "Show window" can bring it back.
                let hidden = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = hidden.hide();
                    }
                });
            }

            build_tray(app.handle())?;

            let result = hid::spawn(move |event| {
                let held = handle.state::<Held>();
                {
                    let mut set = held.0.lock().unwrap();
                    let changed = if event.down {
                        set.insert(event.usage)
                    } else {
                        set.remove(&event.usage)
                    };
                    if !changed {
                        return; // duplicate edge, nothing to draw
                    }
                }
                let _ = handle.emit("key", event);
            });

            // A capture failure is not fatal: the window still comes up and
            // shows the permission prompt so the user can fix it in place.
            if let Err(err) = result {
                eprintln!("[hid] capture unavailable: {err}");
                let _ = app.handle().emit("capture-error", err);
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
