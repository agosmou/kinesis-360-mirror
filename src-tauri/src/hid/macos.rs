//! macOS capture via IOHIDManager.
//!
//! We deliberately do not use a HID convenience crate here. Those open the
//! device to read input *reports*, which for a keyboard means competing with
//! the window server. `IOHIDManagerRegisterInputValueCallback` instead hands us
//! per-element value changes on a device we never exclusively own, so the
//! keyboard keeps working normally while we watch.
//!
//! Requires the Input Monitoring TCC grant (`kTCCServiceListenEvent`), which
//! is a weaker permission than the Accessibility grant a CGEventTap would need.

#![allow(non_snake_case, non_upper_case_globals)]

use std::os::raw::{c_int, c_void};

use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::runloop::CFRunLoop;
use core_foundation::string::{CFString, CFStringRef};

use super::{Access, KeyEvent, KINESIS_IDS, USAGE_PAGE_KEYBOARD};

type IOHIDManagerRef = *mut c_void;
type IOHIDValueRef = *mut c_void;
type IOHIDElementRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFAllocatorRef = *const c_void;
type CFArrayRef = *const c_void;
type IOReturn = c_int;

type IOHIDValueCallback =
    extern "C" fn(context: *mut c_void, result: IOReturn, sender: *mut c_void, value: IOHIDValueRef);

const kIOHIDOptionsTypeNone: u32 = 0;
/// `kIOHIDRequestTypeListenEvent` — observe input, do not synthesize it.
const kIOHIDRequestTypeListenEvent: u32 = 1;
const kIOHIDAccessTypeGranted: u32 = 0;
const kIOHIDAccessTypeDenied: u32 = 1;
const kIOReturnSuccess: IOReturn = 0;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: u32) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatchingMultiple(manager: IOHIDManagerRef, multiple: CFArrayRef);
    fn IOHIDManagerRegisterInputValueCallback(
        manager: IOHIDManagerRef,
        callback: IOHIDValueCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(
        manager: IOHIDManagerRef,
        runLoop: CFRunLoopRef,
        runLoopMode: CFStringRef,
    );
    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: u32) -> IOReturn;
    fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
    fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> isize;
    fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
    fn IOHIDCheckAccess(requestType: u32) -> u32;
    fn IOHIDRequestAccess(requestType: u32) -> bool;
}

extern "C" {
    static kCFRunLoopDefaultMode: CFStringRef;
}

pub fn check_access() -> Access {
    match unsafe { IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) } {
        kIOHIDAccessTypeGranted => Access::Granted,
        kIOHIDAccessTypeDenied => Access::Denied,
        _ => Access::Unknown,
    }
}

/// Triggers the system Input Monitoring prompt the first time it is called.
/// Once the user has denied it, macOS never prompts again and this returns
/// false forever — the UI has to send them to System Settings by hand.
pub fn request_access() -> bool {
    unsafe { IOHIDRequestAccess(kIOHIDRequestTypeListenEvent) }
}

/// Boxed callback, leaked for the lifetime of the run loop.
type Sink = Box<dyn Fn(KeyEvent) + Send + Sync + 'static>;

extern "C" fn on_value(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    value: IOHIDValueRef,
) {
    if context.is_null() || value.is_null() {
        return;
    }
    unsafe {
        let element = IOHIDValueGetElement(value);
        if element.is_null() || IOHIDElementGetUsagePage(element) != USAGE_PAGE_KEYBOARD {
            return;
        }
        let usage = IOHIDElementGetUsage(element);
        // Usages 0..=3 are the error/rollover sentinels, never real keys.
        if usage <= 3 {
            return;
        }
        let sink = &*(context as *const Sink);
        sink(KeyEvent {
            usage,
            down: IOHIDValueGetIntegerValue(value) != 0,
        });
    }
}

fn matching_dicts() -> CFArray<CFDictionary<CFString, CFNumber>> {
    let vendor = CFString::from_static_string("VendorID");
    let product = CFString::from_static_string("ProductID");

    let dicts: Vec<CFDictionary<CFString, CFNumber>> = KINESIS_IDS
        .iter()
        .map(|(vid, pid)| {
            CFDictionary::from_CFType_pairs(&[
                (vendor.clone(), CFNumber::from(*vid as i32)),
                (product.clone(), CFNumber::from(*pid as i32)),
            ])
        })
        .collect();

    CFArray::from_CFTypes(&dicts)
}

/// Spawns the capture thread. It parks on a CFRunLoop forever; the thread is
/// detached and dies with the process.
pub fn spawn<F>(sink: F) -> Result<(), String>
where
    F: Fn(KeyEvent) + Send + Sync + 'static,
{
    if check_access() == Access::Denied {
        return Err("Input Monitoring permission denied".into());
    }

    std::thread::Builder::new()
        .name("kinesis-hid".into())
        .spawn(move || unsafe {
            let manager = IOHIDManagerCreate(std::ptr::null(), kIOHIDOptionsTypeNone);
            if manager.is_null() {
                eprintln!("[hid] IOHIDManagerCreate failed");
                return;
            }

            let dicts = matching_dicts();
            IOHIDManagerSetDeviceMatchingMultiple(manager, dicts.as_CFTypeRef() as CFArrayRef);

            // Leaked on purpose: the run loop below never returns, so the
            // callback context must outlive this scope.
            let boxed: Sink = Box::new(sink);
            let context = Box::into_raw(Box::new(boxed)) as *mut c_void;
            IOHIDManagerRegisterInputValueCallback(manager, on_value, context);

            IOHIDManagerScheduleWithRunLoop(
                manager,
                CFRunLoop::get_current().as_CFTypeRef() as CFRunLoopRef,
                kCFRunLoopDefaultMode,
            );

            let rc = IOHIDManagerOpen(manager, kIOHIDOptionsTypeNone);
            if rc != kIOReturnSuccess {
                // 0xE00002E2 == kIOReturnNotPermitted, the usual cause.
                eprintln!("[hid] IOHIDManagerOpen failed: {:#x}", rc as u32);
                return;
            }

            CFRunLoop::run_current();
        })
        .map_err(|e| format!("failed to spawn capture thread: {e}"))?;

    Ok(())
}
