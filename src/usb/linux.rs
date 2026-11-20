//! Linux and Android: usbfs. Devices are listed from sysfs
//! (`/sys/bus/usb/devices/*/idVendor`, `idProduct`, `busnum`, `devnum`,
//! `descriptors`) and opened as `/dev/bus/usb/BBB/DDD`; transfers are the
//! USBDEVFS_CONTROL and USBDEVFS_BULK ioctls from <linux/usbdevice_fs.h>.
//! Non-root users need a udev rule such as
//! `SUBSYSTEM=="usb", ATTR{idVendor}=="04b4", MODE="0666"`
//! (packaging/linux/60-lumodent-sensor.rules).

use super::{bulk_in_endpoint, Found};
use crate::usb_sensor::Transport;
use std::ffi::c_void;
use std::fs::File;
use std::os::fd::AsRawFd;

extern "C" {
    fn ioctl(fd: i32, request: std::ffi::c_ulong, ...) -> i32;
}

/// `_IOC(dir, 'U', nr, size)` with the asm-generic encoding (x86, ARM, RISC-V).
const fn ioc(dir: u64, nr: u64, size: usize) -> std::ffi::c_ulong {
    ((dir << 30) | ((size as u64) << 16) | ((b'U' as u64) << 8) | nr) as std::ffi::c_ulong
}

#[repr(C)]
struct CtrlTransfer {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    timeout: u32,
    data: *mut c_void,
}

#[repr(C)]
struct BulkTransfer {
    endpoint: u32,
    length: u32,
    timeout: u32,
    data: *mut c_void,
}

const READ_WRITE: u64 = 3;
const READ: u64 = 2;
const USBDEVFS_CONTROL: std::ffi::c_ulong = ioc(READ_WRITE, 0, std::mem::size_of::<CtrlTransfer>());
const USBDEVFS_BULK: std::ffi::c_ulong = ioc(READ_WRITE, 2, std::mem::size_of::<BulkTransfer>());
const USBDEVFS_CLAIMINTERFACE: std::ffi::c_ulong = ioc(READ, 15, 4);
const USBDEVFS_RELEASEINTERFACE: std::ffi::c_ulong = ioc(READ, 16, 4);

fn read_hex(dir: &std::path::Path, name: &str) -> Option<u16> {
    u16::from_str_radix(std::fs::read_to_string(dir.join(name)).ok()?.trim(), 16).ok()
}

fn read_dec(dir: &std::path::Path, name: &str) -> Option<u32> {
    std::fs::read_to_string(dir.join(name)).ok()?.trim().parse().ok()
}

/// Every USB device in sysfs; `path` is its sysfs folder.
pub fn find() -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir("/sys/bus/usb/devices") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            Some(Found { vendor: read_hex(&dir, "idVendor")?, product: read_hex(&dir, "idProduct")?, path: dir.to_string_lossy().into_owned() })
        })
        .collect()
}

pub struct Device {
    file: File,
    bulk_in: u8,
}

impl Device {
    pub fn open(sysfs: &str) -> Result<Self, String> {
        let dir = std::path::Path::new(sysfs);
        let (bus, dev) = (read_dec(dir, "busnum").ok_or("No bus number")?, read_dec(dir, "devnum").ok_or("No device number")?);
        let node = format!("/dev/bus/usb/{bus:03}/{dev:03}");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&node)
            .map_err(|e| format!("Cannot open {node}: {e}. A udev rule for 04b4 may be missing."))?;
        // sysfs "descriptors": the device descriptor followed by the active configuration.
        let desc = std::fs::read(dir.join("descriptors")).map_err(|e| e.to_string())?;
        let bulk_in = bulk_in_endpoint(desc.get(18..).unwrap_or(&[])).ok_or("The sensor has no image endpoint.")?;
        let mut iface: u32 = 0;
        if unsafe { ioctl(file.as_raw_fd(), USBDEVFS_CLAIMINTERFACE, &mut iface as *mut u32) } < 0 {
            return Err(format!("Cannot claim the sensor: {}", std::io::Error::last_os_error()));
        }
        Ok(Device { file, bulk_in })
    }

    fn control(&mut self, request_type: u8, request: u8, data: &mut [u8]) -> Option<usize> {
        let mut t = CtrlTransfer {
            request_type,
            request,
            value: 0,
            index: 0,
            length: data.len() as u16,
            timeout: 10_000,
            data: data.as_mut_ptr() as *mut c_void,
        };
        let n = unsafe { ioctl(self.file.as_raw_fd(), USBDEVFS_CONTROL, &mut t as *mut CtrlTransfer) };
        (n >= 0).then_some(n as usize)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let mut iface: u32 = 0;
        unsafe { ioctl(self.file.as_raw_fd(), USBDEVFS_RELEASEINTERFACE, &mut iface as *mut u32) };
    }
}

impl Transport for Device {
    fn control_in(&mut self, request: u8, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self.control(0xC0, request, &mut buf)?;
        buf.truncate(n);
        Some(buf)
    }

    fn control_out(&mut self, request: u8, data: &[u8]) -> bool {
        let mut buf = data.to_vec();
        self.control(0x40, request, &mut buf).is_some()
    }

    fn bulk_in(&mut self, len: usize, timeout_ms: u32) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let mut t = BulkTransfer { endpoint: self.bulk_in as u32, length: len as u32, timeout: timeout_ms, data: buf.as_mut_ptr() as *mut c_void };
        let n = unsafe { ioctl(self.file.as_raw_fd(), USBDEVFS_BULK, &mut t as *mut BulkTransfer) };
        if n < 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_usbdevice_fs_h() {
        if cfg!(target_pointer_width = "64") {
            assert_eq!(USBDEVFS_CONTROL, 0xC018_5500);
            assert_eq!(USBDEVFS_BULK, 0xC018_5502);
        }
        assert_eq!(USBDEVFS_CLAIMINTERFACE, 0x8004_550F);
    }
}
