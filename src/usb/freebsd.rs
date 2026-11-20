//! FreeBSD: the libusb 1.0 API that ships in the base system (`-lusb`,
//! /usr/lib/libusb.so, FreeBSD's own implementation over ugen). Non-root
//! users need access to the ugen device, e.g. a devd rule or membership of
//! the `operator` group with `devfs` rules for /dev/ugen*.

use super::{bulk_in_endpoint, Found};
use crate::usb_sensor::Transport;
use std::ffi::c_void;

#[repr(C)]
struct DeviceDescriptor {
    length: u8,
    descriptor_type: u8,
    bcd_usb: u16,
    class: u8,
    subclass: u8,
    protocol: u8,
    max_packet0: u8,
    vendor: u16,
    product: u16,
    bcd_device: u16,
    manufacturer: u8,
    product_index: u8,
    serial: u8,
    configurations: u8,
}

#[link(name = "usb")]
extern "C" {
    fn libusb_init(ctx: *mut *mut c_void) -> i32;
    fn libusb_get_device_list(ctx: *mut c_void, list: *mut *mut *mut c_void) -> isize;
    fn libusb_free_device_list(list: *mut *mut c_void, unref: i32);
    fn libusb_get_device_descriptor(dev: *mut c_void, desc: *mut DeviceDescriptor) -> i32;
    fn libusb_get_bus_number(dev: *mut c_void) -> u8;
    fn libusb_get_device_address(dev: *mut c_void) -> u8;
    fn libusb_open(dev: *mut c_void, handle: *mut *mut c_void) -> i32;
    fn libusb_close(handle: *mut c_void);
    fn libusb_claim_interface(handle: *mut c_void, iface: i32) -> i32;
    fn libusb_release_interface(handle: *mut c_void, iface: i32) -> i32;
    fn libusb_control_transfer(handle: *mut c_void, request_type: u8, request: u8, value: u16, index: u16, data: *mut u8, len: u16, timeout: u32) -> i32;
    fn libusb_bulk_transfer(handle: *mut c_void, endpoint: u8, data: *mut u8, len: i32, transferred: *mut i32, timeout: u32) -> i32;
}

/// The process-wide libusb context (created once, never freed).
fn context() -> *mut c_void {
    use std::sync::OnceLock;
    static CTX: OnceLock<usize> = OnceLock::new();
    *CTX.get_or_init(|| {
        let mut ctx = std::ptr::null_mut();
        if unsafe { libusb_init(&mut ctx) } == 0 { ctx as usize } else { 0 }
    }) as *mut c_void
}

/// Runs `f` on every device in the list until it returns Some.
fn each_device<T>(mut f: impl FnMut(*mut c_void, &DeviceDescriptor) -> Option<T>) -> Vec<T> {
    let mut out = Vec::new();
    let ctx = context();
    if ctx.is_null() {
        return out;
    }
    unsafe {
        let mut list = std::ptr::null_mut();
        let n = libusb_get_device_list(ctx, &mut list);
        for i in 0..n.max(0) as usize {
            let dev = *list.add(i);
            let mut d = std::mem::zeroed::<DeviceDescriptor>();
            if libusb_get_device_descriptor(dev, &mut d) == 0 {
                if let Some(v) = f(dev, &d) {
                    out.push(v);
                }
            }
        }
        if !list.is_null() {
            libusb_free_device_list(list, 1);
        }
    }
    out
}

fn bus_path(dev: *mut c_void) -> String {
    unsafe { format!("{}.{}", libusb_get_bus_number(dev), libusb_get_device_address(dev)) }
}

/// Every USB device; `path` is "bus.address".
pub fn find() -> Vec<Found> {
    each_device(|dev, d| Some(Found { path: bus_path(dev), vendor: d.vendor, product: d.product }))
}

pub struct Device {
    handle: *mut c_void,
    bulk_in: u8,
}

// The handle is only used from the thread that owns the Device.
unsafe impl Send for Device {}

impl Device {
    pub fn open(found: &Found) -> Result<Self, String> {
        let handles = each_device(|dev, _| {
            if bus_path(dev) != found.path {
                return None;
            }
            let mut h = std::ptr::null_mut();
            (unsafe { libusb_open(dev, &mut h) } == 0).then_some(h as usize)
        });
        let handle = *handles.first().ok_or("Cannot open the sensor (check /dev/ugen permissions).")? as *mut c_void;
        let mut dev = Device { handle, bulk_in: 0 };
        let mut head = [0u8; 9];
        dev.control(0x80, 6, 0x0200, &mut head).ok_or("Cannot read the sensor's configuration.")?;
        let mut config = vec![0u8; u16::from_le_bytes([head[2], head[3]]).max(9) as usize];
        dev.control(0x80, 6, 0x0200, &mut config).ok_or("Cannot read the sensor's configuration.")?;
        dev.bulk_in = bulk_in_endpoint(&config).ok_or("The sensor has no image endpoint.")?;
        if unsafe { libusb_claim_interface(handle, 0) } != 0 {
            return Err("Cannot claim the sensor.".into());
        }
        Ok(dev)
    }

    fn control(&mut self, request_type: u8, request: u8, value: u16, data: &mut [u8]) -> Option<usize> {
        let n = unsafe { libusb_control_transfer(self.handle, request_type, request, value, 0, data.as_mut_ptr(), data.len() as u16, 10_000) };
        (n >= 0).then_some(n as usize)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            libusb_release_interface(self.handle, 0);
            libusb_close(self.handle);
        }
    }
}

impl Transport for Device {
    fn control_in(&mut self, request: u8, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self.control(0xC0, request, 0, &mut buf)?;
        buf.truncate(n);
        Some(buf)
    }

    fn control_out(&mut self, request: u8, data: &[u8]) -> bool {
        let mut buf = data.to_vec();
        self.control(0x40, request, 0, &mut buf).is_some()
    }

    fn bulk_in(&mut self, len: usize, timeout_ms: u32) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let mut done = 0i32;
        let r = unsafe { libusb_bulk_transfer(self.handle, self.bulk_in, buf.as_mut_ptr(), len as i32, &mut done, timeout_ms) };
        // A timeout after some data still returns what arrived, like CyAPI.
        if r != 0 && done == 0 {
            return None;
        }
        buf.truncate(done.max(0) as usize);
        Some(buf)
    }
}
