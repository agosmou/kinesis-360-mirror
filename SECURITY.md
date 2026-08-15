# Security

This app watches your keystrokes. That is its function, and it is also exactly
what a keylogger does. You should not take anyone's word for what it does with
them — including mine. This document exists so you can check.

## What it does with keystrokes

Nothing beyond drawing them.

A key event goes: HID callback → a `HashSet` of currently-held usages (so a
key held down is not drawn twice) → an event to the window → a CSS class on an
SVG rectangle. Then it is gone. There is no buffer, no log, no counter, no
history.

Concretely, the app:

- **never writes keystrokes to disk.** There is no logging of input, at any
  verbosity. The only thing persisted anywhere is your window width, in
  `localStorage`.
- **never sends anything over the network.** There is no networking code in the
  binary at all — no HTTP client, no sockets, no telemetry, no crash reporting,
  no update check.
- **cannot type.** See below.
- **only sees the Kinesis.** Capture is filtered to the Kinesis vendor/product
  IDs. Your laptop keyboard, your password manager's field, another USB
  keyboard — none of it reaches this app.

## Verify that yourself

Do not trust the paragraph above. These take a minute:

```bash
# No networking anywhere in the Rust source.
grep -rnE "reqwest|hyper|ureq|TcpStream|UdpSocket|std::net|http" src-tauri/src

# No file writes. The only filesystem access is reading /dev/input on Linux.
grep -rnE "File::create|fs::write|OpenOptions|write_all" src-tauri/src

# Nothing stored in the frontend but a window width.
grep -rnE "localStorage|indexedDB|fetch\(" src/main.js
```

For the shipped binary rather than the source:

```bash
BIN=/Applications/kinesis-360-mirror.app/Contents/MacOS/kinesis-360-mirror

# It does not even link a networking framework. No CFNetwork, no Network.
otool -L "$BIN" | grep -iE "Network|CFNetwork"

# No socket syscalls imported.
nm -u "$BIN" | grep -iE "_socket$|_connect$|_send$|_bind$"

# Zero connections while it runs. Type on the board first, then check.
lsof -i -a -p "$(pgrep -f 'kinesis-360-mirror.app/Contents/MacOS')"
```

All three come back empty. The first is the strongest: a binary that cannot
resolve a networking symbol cannot phone home, regardless of what its source
says.

Or just read [`src-tauri/src/hid/`](src-tauri/src/hid/) — the capture layer is
about 400 lines across both platforms.

## It cannot type on your behalf

This is structural, not a promise.

macOS splits input access into two separate permissions. `IOHIDCheckAccess`
takes a request type: `kIOHIDRequestTypeListenEvent` (observe input) and
`kIOHIDRequestTypePostEvent` (synthesize input). This app only ever requests
**ListenEvent**. It never links or calls `IOHIDPostEvent`, `CGEventPost`, or
any other injection API, and it does not hold the Accessibility permission that
would be required.

So even a compromised build of this app, running with the permission you
granted it, cannot press a key. Granting it Input Monitoring does not grant it
the ability to act as you.

The on-screen keys are not buttons. Clicking one does nothing — the SVG has
`pointer-events: none`.

## Permissions it asks for, and why

| Permission | Platform | Why |
| --- | --- | --- |
| Input Monitoring (`kTCCServiceListenEvent`) | macOS | Read HID usages from the keyboard. This is the weaker of the two input permissions; a `CGEventTap` approach would have needed Accessibility, which is far more powerful. |
| Read on `/dev/input/event*` | Linux | Same, via evdev. Granted with a udev rule scoped to the Kinesis IDs, not by adding you to the `input` group. |

It requests no others. It does not ask for Accessibility, Full Disk Access,
camera, microphone, or any network entitlement.

It is not sandboxed, and cannot be: the App Sandbox does not permit the global
input monitoring this app is built on. That is also why it is distributed as a
signed, notarized DMG rather than through an app store.

## Attack surface

- **IPC commands.** Six, all trivial: query permission state, request
  permission, toggle click-through, toggle always-on-top, resize, and log a
  string to stderr. None take a path, a command, or a URL. `resize_window`
  rejects non-finite values and clamps; `ui_log` strips control characters and
  truncates so it cannot inject terminal escapes.
- **Content Security Policy.** Strict, `default-src 'self'`. No remote origins
  are reachable from the webview.
- **Plugins.** None. The Tauri scaffold ships `tauri-plugin-opener`, which can
  open arbitrary URLs and files; it was never used here and has been removed,
  along with its capability grant.
- **`unsafe` code.** Four blocks, all in `src-tauri/src/hid/macos.rs`, all
  calls into IOKit. The one non-obvious thing is a deliberately leaked callback
  context: it must outlive a `CFRunLoop` that never returns.
- **Dependencies.** `cargo audit` reports **0 vulnerabilities**. The
  warnings it does report are "unmaintained" advisories for GTK3 bindings that
  Tauri pulls in for its Linux backend; they are not reachable from this code
  and are not fixable here.

## Note on the debug tools

Two things exist for development and *do* surface keystrokes:

- `cargo run --example probe` prints raw HID usages to your terminal for 10
  seconds. That is the point of it — it is how you tell "capture is broken"
  from "the UI is broken". It is not part of the app bundle.
- `__mirror.onKey(...)` in the web inspector drives the board with synthetic
  events. It reads nothing.

Neither writes to disk or the network.

## Building it yourself

The most reliable way to trust a binary is not to use mine. `bun run
install:mac` builds from source and installs to `/Applications`. Releases are
signed with a Developer ID certificate and notarized by Apple, which proves the
build has not been tampered with in transit — it does not prove the source
matches, which is why the checks above are worth a minute.

## Reporting a problem

Open a GitHub issue for anything non-sensitive. For something you would rather
not post publicly, use GitHub's private vulnerability reporting on this
repository.

This is a personal project with no SLA. It is also ~1500 lines with no network
code, so the realistic blast radius of a bug here is small.
