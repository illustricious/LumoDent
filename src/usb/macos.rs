//! macOS: IOKit's USB user client (IOUSBLib), called through the plug-in
//! interfaces directly. Devices are matched by class (IOUSBHostDevice, and
//! IOUSBDevice on older systems) and identified by registry entry ID. Control
//! transfers use IOUSBDeviceInterface::DeviceRequest and the image comes from
//! IOUSBInterfaceInterface::ReadPipe; a watchdog aborts the pipe after the
//! timeout because the base interface has no timed read. No driver or
//! entitlement is needed for a vendor-specific device.

use super::Found;
use crate::usb_sensor::Transport;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

type CFTypeRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct UuidBytes([u8; 16]);

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingServices(port: u32, matching: *mut c_void, iter: *mut u32) -> i32;
    fn IOServiceGetMatchingService(port: u32, matching: *mut c_void) -> u32;
    fn IORegistryEntryIDMatching(id: u64) -> *mut c_void;
    fn IORegistryEntryGetRegistryEntryID(entry: u32, id: *mut u64) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: CFTypeRef, alloc: CFTypeRef, options: u32) -> CFTypeRef;
    fn IOIteratorNext(iter: u32) -> u32;
    fn IOObjectRelease(obj: u32) -> i32;
    fn IOCreatePlugInInterfaceForService(service: u32, plugin_type: CFTypeRef, interface_type: CFTypeRef, plugin: *mut *mut *mut c_void, score: *mut i32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFTypeRef;
    fn CFNumberGetValue(n: CFTypeRef, kind: i32, out: *mut c_void) -> u8;
    fn CFRelease(obj: CFTypeRef);
    fn CFUUIDGetConstantUUIDWithBytes(
        alloc: CFTypeRef, b0: u8, b1: u8, b2: u8, b3: u8, b4: u8, b5: u8, b6: u8, b7: u8, b8: u8, b9: u8, b10: u8, b11: u8, b12: u8, b13: u8, b14: u8, b15: u8,
    ) -> CFTypeRef;
}

fn uuid(b: [u8; 16]) -> CFTypeRef {
    unsafe { CFUUIDGetConstantUUIDWithBytes(std::ptr::null(), b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]) }
}

// From IOKit/usb/IOUSBLib.h and IOKit/IOCFPlugIn.h.
const DEVICE_USER_CLIENT: [u8; 16] = [0x9d, 0xc7, 0xb7, 0x80, 0x9e, 0xc0, 0x11, 0xd4, 0xa5, 0x4f, 0x00, 0x0a, 0x27, 0x05, 0x28, 0x61];
const INTERFACE_USER_CLIENT: [u8; 16] = [0x2d, 0x97, 0x86, 0xc6, 0x9e, 0xf3, 0x11, 0xd4, 0xad, 0x51, 0x00, 0x0a, 0x27, 0x05, 0x28, 0x61];
const CF_PLUGIN: [u8; 16] = [0xc2, 0x44, 0xe8, 0x58, 0x10, 0x9c, 0x11, 0xd4, 0x91, 0xd4, 0x00, 0x50, 0xe4, 0xc6, 0x42, 0x6f];
const DEVICE_INTERFACE: [u8; 16] = [0x5c, 0x81, 0x87, 0xd0, 0x9e, 0xf3, 0x11, 0xd4, 0x8b, 0x45, 0x00, 0x0a, 0x27, 0x05, 0x28, 0x61];
const INTERFACE_INTERFACE: [u8; 16] = [0x73, 0xc9, 0x7a, 0xe8, 0x9e, 0xf3, 0x11, 0xd4, 0xb1, 0xd0, 0x00, 0x0a, 0x27, 0x05, 0x28, 0x61];

/// A COM-style object: a pointer to a pointer to its function table.
type Obj = *mut *mut c_void;

/// Function `index` of an object's table.
unsafe fn slot<F: Copy>(obj: Obj, index: usize) -> F {
    let table = *obj as *const *const c_void;
    std::mem::transmute_copy(&*table.add(index))
}

// Table positions (IUnknown first: 0 reserved, 1 QueryInterface, 2 AddRef, 3 Release).
const QUERY_INTERFACE: usize = 1;
const RELEASE: usize = 3;
const DEV_OPEN: usize = 8;
const DEV_CLOSE: usize = 9;
const DEV_SET_CONFIGURATION: usize = 23;
const DEV_REQUEST: usize = 26;
const DEV_INTERFACE_ITERATOR: usize = 28;
const IF_OPEN: usize = 8;
const IF_CLOSE: usize = 9;
const IF_NUM_ENDPOINTS: usize = 19;
const IF_PIPE_PROPERTIES: usize = 26;
const IF_ABORT_PIPE: usize = 28;
const IF_READ_PIPE: usize = 31;

#[repr(C)]
struct DevRequest {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    data: *mut c_void,
    done: u32,
}

#[repr(C)]
struct FindInterface {
    class: u16,
    subclass: u16,
    protocol: u16,
    alternate: u16,
}

fn number(entry: u32, key: &str) -> Option<u16> {
    let key = CString::new(key).ok()?;
    unsafe {
        let k = CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), 0x0800_0100);
        let v = IORegistryEntryCreateCFProperty(entry, k, std::ptr::null(), 0);
        CFRelease(k);
        if v.is_null() {
            return None;
        }
        let mut n: i32 = 0;
        let ok = CFNumberGetValue(v, 3, &mut n as *mut i32 as *mut c_void) != 0;
        CFRelease(v);
        ok.then_some(n as u16)
    }
}

/// Every USB device; `path` is its registry entry ID.
pub fn find() -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for class in ["IOUSBHostDevice", "IOUSBDevice"] {
        let name = CString::new(class).unwrap();
        unsafe {
            let mut iter = 0u32;
            if IOServiceGetMatchingServices(0, IOServiceMatching(name.as_ptr()), &mut iter) != 0 {
                continue;
            }
            loop {
                let entry = IOIteratorNext(iter);
                if entry == 0 {
                    break;
                }
                let mut id = 0u64;
                IORegistryEntryGetRegistryEntryID(entry, &mut id);
                if let (Some(vendor), Some(product)) = (number(entry, "idVendor"), number(entry, "idProduct")) {
                    let path = id.to_string();
                    if !out.iter().any(|f| f.path == path) {
                        out.push(Found { path, vendor, product });
                    }
                }
                IOObjectRelease(entry);
            }
            IOObjectRelease(iter);
        }
    }
    out
}

/// Creates the user client of `service` and asks it for `interface`.
unsafe fn plug_in(service: u32, client: [u8; 16], interface: [u8; 16]) -> Option<Obj> {
    let mut plugin: *mut *mut c_void = std::ptr::null_mut();
    let mut score = 0i32;
    if IOCreatePlugInInterfaceForService(service, uuid(client), uuid(CF_PLUGIN), &mut plugin, &mut score) != 0 || plugin.is_null() {
        return None;
    }
    let query: unsafe extern "C" fn(Obj, UuidBytes, *mut Obj) -> i32 = slot(plugin, QUERY_INTERFACE);
    let release: unsafe extern "C" fn(Obj) -> u32 = slot(plugin, RELEASE);
    let mut obj: Obj = std::ptr::null_mut();
    let r = query(plugin, UuidBytes(interface), &mut obj);
    release(plugin);
    (r == 0 && !obj.is_null()).then_some(obj)
}

pub struct Device {
    dev: Obj,
    intf: Obj,
    pipe: u8,
}

// IOKit user-client objects may be used from any one thread at a time.
unsafe impl Send for Device {}

impl Device {
    pub fn open(found: &Found) -> Result<Self, String> {
        let id: u64 = found.path.parse().map_err(|_| "Bad device ID")?;
        unsafe {
            let service = IOServiceGetMatchingService(0, IORegistryEntryIDMatching(id));
            if service == 0 {
                return Err("The sensor was unplugged.".into());
            }
            let dev = plug_in(service, DEVICE_USER_CLIENT, DEVICE_INTERFACE);
            IOObjectRelease(service);
            let dev = dev.ok_or("Cannot reach the sensor through IOKit.")?;
            let open: unsafe extern "C" fn(Obj) -> i32 = slot(dev, DEV_OPEN);
            if open(dev) != 0 {
                let release: unsafe extern "C" fn(Obj) -> u32 = slot(dev, RELEASE);
                release(dev);
                return Err("The sensor is in use by another program.".into());
            }
            let set_config: unsafe extern "C" fn(Obj, u8) -> i32 = slot(dev, DEV_SET_CONFIGURATION);
            set_config(dev, 1);
            let iterator: unsafe extern "C" fn(Obj, *mut FindInterface, *mut u32) -> i32 = slot(dev, DEV_INTERFACE_ITERATOR);
            let mut req = FindInterface { class: 0xffff, subclass: 0xffff, protocol: 0xffff, alternate: 0xffff };
            let mut iter = 0u32;
            let mut device = Device { dev, intf: std::ptr::null_mut(), pipe: 0 };
            if iterator(dev, &mut req, &mut iter) != 0 {
                return Err("The sensor has no interface.".into());
            }
            let first = IOIteratorNext(iter);
            IOObjectRelease(iter);
            if first == 0 {
                return Err("The sensor has no interface.".into());
            }
            let intf = plug_in(first, INTERFACE_USER_CLIENT, INTERFACE_INTERFACE);
            IOObjectRelease(first);
            device.intf = intf.ok_or("Cannot open the sensor's interface.")?;
            let if_open: unsafe extern "C" fn(Obj) -> i32 = slot(device.intf, IF_OPEN);
            if if_open(device.intf) != 0 {
                return Err("Cannot open the sensor's interface.".into());
            }
            let count: unsafe extern "C" fn(Obj, *mut u8) -> i32 = slot(device.intf, IF_NUM_ENDPOINTS);
            let props: unsafe extern "C" fn(Obj, u8, *mut u8, *mut u8, *mut u8, *mut u16, *mut u8) -> i32 = slot(device.intf, IF_PIPE_PROPERTIES);
            let mut n = 0u8;
            count(device.intf, &mut n);
            for pipe in 1..=n {
                let (mut dir, mut num, mut kind, mut size, mut interval) = (0u8, 0u8, 0u8, 0u16, 0u8);
                if props(device.intf, pipe, &mut dir, &mut num, &mut kind, &mut size, &mut interval) == 0 && dir == 1 && kind == 2 {
                    device.pipe = pipe;
                    break;
                }
            }
            if device.pipe == 0 {
                return Err("The sensor has no image endpoint.".into());
            }
            Ok(device)
        }
    }

    fn request(&mut self, request_type: u8, request: u8, data: &mut [u8]) -> Option<usize> {
        let mut r = DevRequest { request_type, request, value: 0, index: 0, length: data.len() as u16, data: data.as_mut_ptr() as *mut c_void, done: 0 };
        let f: unsafe extern "C" fn(Obj, *mut DevRequest) -> i32 = unsafe { slot(self.dev, DEV_REQUEST) };
        (unsafe { f(self.dev, &mut r) } == 0).then_some(r.done as usize)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            if !self.intf.is_null() {
                let close: unsafe extern "C" fn(Obj) -> i32 = slot(self.intf, IF_CLOSE);
                let release: unsafe extern "C" fn(Obj) -> u32 = slot(self.intf, RELEASE);
                close(self.intf);
                release(self.intf);
            }
            let close: unsafe extern "C" fn(Obj) -> i32 = slot(self.dev, DEV_CLOSE);
            let release: unsafe extern "C" fn(Obj) -> u32 = slot(self.dev, RELEASE);
            close(self.dev);
            release(self.dev);
        }
    }
}

impl Transport for Device {
    fn control_in(&mut self, request: u8, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self.request(0xC0, request, &mut buf)?;
        buf.truncate(n);
        Some(buf)
    }

    fn control_out(&mut self, request: u8, data: &[u8]) -> bool {
        let mut buf = data.to_vec();
        self.request(0x40, request, &mut buf).is_some()
    }

    fn bulk_in(&mut self, len: usize, timeout_ms: u32) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let mut size = len as u32;
        let done = Arc::new(AtomicBool::new(false));
        // Abort the read if it runs past the timeout.
        let (flag, intf, pipe) = (done.clone(), self.intf as usize, self.pipe);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
            if !flag.load(Ordering::SeqCst) {
                unsafe {
                    let abort: unsafe extern "C" fn(Obj, u8) -> i32 = slot(intf as Obj, IF_ABORT_PIPE);
                    abort(intf as Obj, pipe);
                }
            }
        });
        let read: unsafe extern "C" fn(Obj, u8, *mut c_void, *mut u32) -> i32 = unsafe { slot(self.intf, IF_READ_PIPE) };
        let r = unsafe { read(self.intf, self.pipe, buf.as_mut_ptr() as *mut c_void, &mut size) };
        done.store(true, Ordering::SeqCst);
        if r != 0 {
            return None;
        }
        buf.truncate(size as usize);
        Some(buf)
    }
}
