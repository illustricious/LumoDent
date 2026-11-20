//! Finding and opening the sensor on each operating system. The sensor_io
//! protocol (src/usb_sensor.rs) only needs vendor control transfers and bulk
//! IN reads; this module provides them per platform, with no USB library
//! beyond what the system ships:
//!
//! - Windows: the installed NanoPix driver (CyUSB I/O controls), src/cyusb.rs.
//! - Linux and Android: the kernel's usbfs (`/dev/bus/usb`), no driver needed.
//! - FreeBSD: the base system's libusb (`/usr/lib/libusb.so`).
//! - macOS: IOKit's USB user client.
//!
//! Only the Windows path has a sensor to answer it so far; the others follow
//! each system's documented interfaces and still need a test with a sensor.

use crate::usb_sensor::{Transport, PRODUCT_IDS, VENDOR_ID};

#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux;
#[cfg(target_os = "freebsd")]
mod freebsd;
#[cfg(target_os = "macos")]
mod macos;

/// A sensor found on the bus. `path` is whatever the platform opens it by.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub path: String,
    pub vendor: u16,
    pub product: u16,
}

/// True for the IDs the NanoPix driver binds (VID 04B4, PID 0042-0046).
pub fn is_sensor(vendor: u16, product: u16) -> bool {
    vendor == VENDOR_ID && PRODUCT_IDS.contains(&product)
}

/// The first bulk IN endpoint of a configuration descriptor.
pub fn bulk_in_endpoint(config: &[u8]) -> Option<u8> {
    let mut i = 0;
    while i + 1 < config.len() {
        let (len, kind) = (config[i] as usize, config[i + 1]);
        if len < 2 {
            return None;
        }
        if kind == 5 && i + 3 < config.len() && config[i + 2] & 0x80 != 0 && config[i + 3] & 3 == 2 {
            return Some(config[i + 2]);
        }
        i += len;
    }
    None
}

/// Sensors present now.
pub fn find_sensors() -> Vec<Found> {
    let all: Vec<Found> = {
        #[cfg(windows)]
        {
            crate::cyusb::find()
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            linux::find()
        }
        #[cfg(target_os = "freebsd")]
        {
            freebsd::find()
        }
        #[cfg(target_os = "macos")]
        {
            macos::find()
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "android", target_os = "freebsd", target_os = "macos")))]
        {
            Vec::new()
        }
    };
    all.into_iter().filter(|d| is_sensor(d.vendor, d.product)).collect()
}

/// Opens a sensor from [`find_sensors`].
pub fn open(found: &Found) -> Result<Box<dyn Transport + Send>, String> {
    #[cfg(windows)]
    {
        crate::cyusb::CyDevice::open(&found.path).map(|d| Box::new(d) as Box<dyn Transport + Send>)
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        linux::Device::open(&found.path).map(|d| Box::new(d) as Box<dyn Transport + Send>)
    }
    #[cfg(target_os = "freebsd")]
    {
        freebsd::Device::open(found).map(|d| Box::new(d) as Box<dyn Transport + Send>)
    }
    #[cfg(target_os = "macos")]
    {
        macos::Device::open(found).map(|d| Box::new(d) as Box<dyn Transport + Send>)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android", target_os = "freebsd", target_os = "macos")))]
    {
        let _ = found;
        Err("USB sensors are not supported on this system.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_ids_and_endpoint() {
        assert!(is_sensor(0x04B4, 0x0044) && !is_sensor(0x04B4, 0x8613) && !is_sensor(0x1234, 0x0044));
        let mut c = vec![9, 2, 32, 0, 1, 1, 0, 0x80, 50];
        c.extend([9, 4, 0, 0, 2, 0xff, 0, 0, 0]);
        c.extend([7, 5, 0x02, 2, 0, 2, 0]);
        c.extend([7, 5, 0x86, 2, 0, 2, 0]);
        assert_eq!(bulk_in_endpoint(&c), Some(0x86));
        assert_eq!(bulk_in_endpoint(&c[..25]), None);
    }
}
