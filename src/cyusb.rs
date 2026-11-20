//! The sensor's USB link through its installed driver.
//!
//! The NanoPix sensor driver (IntraoralImagingSensor.sys, a CyUSB build)
//! publishes the CyUSB device interface {AE18AA60-7F6A-11d4-97DD-00010229B959}
//! and takes the CyUSB I/O controls. sensor_io.dll reaches it through CyAPI;
//! this file does the same with SetupAPI and DeviceIoControl, so no Cypress
//! library is needed. Request layout, as built by CyAPI in sensor_io.dll:
//! a 0x26-byte SINGLE_TRANSFER header (setup packet, endpoint address at
//! +0x0D, NT/USBD status at +0x0E/+0x12, buffer offset +0x1E and length
//! +0x22) followed by the data, sent with IOCTL 0x220020 for endpoint 0 and
//! 0x220024 for the bulk endpoint.

pub use crate::usb::Found;
#[cfg(windows)]
use crate::usb::bulk_in_endpoint;
use crate::usb_sensor::Transport;

pub const IOCTL_EP0: u32 = 0x0022_0020;
pub const IOCTL_BULK: u32 = 0x0022_0024;
pub const HEADER: usize = 0x26;

/// One SINGLE_TRANSFER request for endpoint 0. `request_type` is the USB
/// bmRequestType (0xC0 vendor IN, 0x40 vendor OUT, 0x80 standard IN).
pub fn control_request(request_type: u8, request: u8, value: u16, index: u16, data: &[u8], len: usize, timeout_s: u32) -> Vec<u8> {
    let n = if request_type & 0x80 != 0 { len } else { data.len() };
    let mut b = vec![0u8; HEADER + n];
    b[0] = request_type;
    b[1] = request;
    b[2..4].copy_from_slice(&value.to_le_bytes());
    b[4..6].copy_from_slice(&index.to_le_bytes());
    b[6..8].copy_from_slice(&(n as u16).to_le_bytes());
    b[8..12].copy_from_slice(&timeout_s.to_le_bytes());
    b[0x1e..0x22].copy_from_slice(&(HEADER as u32).to_le_bytes());
    b[0x22..0x26].copy_from_slice(&(n as u32).to_le_bytes());
    if request_type & 0x80 == 0 {
        b[HEADER..].copy_from_slice(data);
    }
    b
}

/// One SINGLE_TRANSFER request for a bulk endpoint.
pub fn bulk_request(endpoint: u8, len: usize) -> Vec<u8> {
    let mut b = vec![0u8; HEADER + len];
    b[0x0d] = endpoint;
    b[0x1e..0x22].copy_from_slice(&(HEADER as u32).to_le_bytes());
    b[0x22..0x26].copy_from_slice(&(len as u32).to_le_bytes());
    b
}

/// The data part of a completed request: `BufferLength` bytes after the header.
pub fn reply_data(b: &[u8]) -> Vec<u8> {
    let len = b.get(0x22..0x26).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize).unwrap_or(0);
    b.get(HEADER..HEADER + len.min(b.len().saturating_sub(HEADER))).unwrap_or(&[]).to_vec()
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);
    const CYUSB: Guid = Guid(0xAE18AA60, 0x7F6A, 0x11d4, [0x97, 0xDD, 0x00, 0x01, 0x02, 0x29, 0xB9, 0x59]);

    #[repr(C)]
    struct InterfaceData {
        size: u32,
        class: Guid,
        flags: u32,
        reserved: usize,
    }

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: isize,
    }

    #[link(name = "setupapi", kind = "raw-dylib")]
    extern "system" {
        fn SetupDiGetClassDevsW(guid: *const Guid, enumerator: *const u16, parent: isize, flags: u32) -> isize;
        fn SetupDiEnumDeviceInterfaces(set: isize, info: *const c_void, guid: *const Guid, index: u32, data: *mut InterfaceData) -> i32;
        fn SetupDiGetDeviceInterfaceDetailW(set: isize, data: *const InterfaceData, detail: *mut u8, size: u32, required: *mut u32, info: *mut c_void) -> i32;
        fn SetupDiDestroyDeviceInfoList(set: isize) -> i32;
    }

    #[link(name = "kernel32", kind = "raw-dylib")]
    extern "system" {
        fn CreateFileW(name: *const u16, access: u32, share: u32, security: *const c_void, disposition: u32, flags: u32, template: isize) -> isize;
        fn DeviceIoControl(h: isize, code: u32, input: *const u8, in_len: u32, output: *mut u8, out_len: u32, returned: *mut u32, ov: *mut Overlapped) -> i32;
        fn GetOverlappedResult(h: isize, ov: *mut Overlapped, transferred: *mut u32, wait: i32) -> i32;
        fn CancelIoEx(h: isize, ov: *mut Overlapped) -> i32;
        fn CreateEventW(security: *const c_void, manual: i32, initial: i32, name: *const u16) -> isize;
        fn WaitForSingleObject(h: isize, ms: u32) -> u32;
        fn CloseHandle(h: isize) -> i32;
        fn GetLastError() -> u32;
    }

    const INVALID: isize = -1;

    /// Device paths of every present CyUSB-interface device.
    pub fn paths() -> Vec<String> {
        let mut out = Vec::new();
        unsafe {
            let set = SetupDiGetClassDevsW(&CYUSB, std::ptr::null(), 0, 0x02 | 0x10);
            if set == INVALID {
                return out;
            }
            for index in 0.. {
                let mut data = InterfaceData { size: std::mem::size_of::<InterfaceData>() as u32, class: Guid(0, 0, 0, [0; 8]), flags: 0, reserved: 0 };
                if SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &CYUSB, index, &mut data) == 0 {
                    break;
                }
                let mut need = 0u32;
                SetupDiGetDeviceInterfaceDetailW(set, &data, std::ptr::null_mut(), 0, &mut need, std::ptr::null_mut());
                if need < 8 {
                    continue;
                }
                let mut buf = vec![0u8; need as usize];
                // cbSize of SP_DEVICE_INTERFACE_DETAIL_DATA_W: 8 on 64-bit, 6 on 32-bit.
                let size: u32 = if cfg!(target_pointer_width = "64") { 8 } else { 6 };
                buf[..4].copy_from_slice(&size.to_le_bytes());
                if SetupDiGetDeviceInterfaceDetailW(set, &data, buf.as_mut_ptr(), need, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                    continue;
                }
                let wide: Vec<u16> = buf[4..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
                out.push(String::from_utf16_lossy(&wide));
            }
            SetupDiDestroyDeviceInfoList(set);
        }
        out
    }

    pub struct Handle(pub isize);

    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    impl Handle {
        pub fn open(path: &str) -> Result<Self, String> {
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            // GENERIC_READ | GENERIC_WRITE, shared, OPEN_EXISTING, FILE_FLAG_OVERLAPPED.
            let h = unsafe { CreateFileW(wide.as_ptr(), 0xC000_0000, 3, std::ptr::null(), 3, 0x4000_0000, 0) };
            if h == INVALID {
                return Err(format!("Cannot open the sensor (error {}).", unsafe { GetLastError() }));
            }
            Ok(Handle(h))
        }

        /// Sends a request and waits for it, cancelling after `timeout_ms`.
        pub fn ioctl(&self, code: u32, buf: &mut [u8], timeout_ms: u32) -> bool {
            unsafe {
                let event = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
                if event == 0 {
                    return false;
                }
                let mut ov = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event };
                let mut done = 0u32;
                let len = buf.len() as u32;
                let mut ok = DeviceIoControl(self.0, code, buf.as_ptr(), len, buf.as_mut_ptr(), len, &mut done, &mut ov) != 0;
                if !ok && GetLastError() == 997 {
                    // ERROR_IO_PENDING
                    if WaitForSingleObject(event, timeout_ms) != 0 {
                        CancelIoEx(self.0, &mut ov);
                    }
                    ok = GetOverlappedResult(self.0, &mut ov, &mut done, 1) != 0;
                }
                CloseHandle(event);
                ok
            }
        }
    }
}

/// Lists sensors bound to the driver, with their USB IDs.
pub fn find() -> Vec<Found> {
    #[cfg(windows)]
    {
        win::paths()
            .into_iter()
            .filter_map(|path| {
                let dev = CyDevice::open(&path).ok()?;
                Some(Found { path, vendor: dev.vendor, product: dev.product })
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// An opened sensor, implementing the transport the protocol uses.
pub struct CyDevice {
    #[cfg(windows)]
    handle: win::Handle,
    pub vendor: u16,
    pub product: u16,
    pub bulk_in: u8,
}

impl CyDevice {
    #[cfg(windows)]
    pub fn open(path: &str) -> Result<Self, String> {
        let mut dev = CyDevice { handle: win::Handle::open(path)?, vendor: 0, product: 0, bulk_in: 0 };
        let desc = dev.standard_in(0x0100, 18).ok_or("The sensor does not answer.")?;
        if desc.len() >= 12 {
            dev.vendor = u16::from_le_bytes([desc[8], desc[9]]);
            dev.product = u16::from_le_bytes([desc[10], desc[11]]);
        }
        let head = dev.standard_in(0x0200, 9).ok_or("Cannot read the sensor's configuration.")?;
        let total = head.get(2..4).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).unwrap_or(9).max(9);
        let config = dev.standard_in(0x0200, total).ok_or("Cannot read the sensor's configuration.")?;
        dev.bulk_in = bulk_in_endpoint(&config).ok_or("The sensor has no image endpoint.")?;
        Ok(dev)
    }

    #[cfg(not(windows))]
    pub fn open(_path: &str) -> Result<Self, String> {
        Err("The sensor driver is only available on Windows.".into())
    }

    /// GET_DESCRIPTOR from endpoint 0.
    #[cfg(windows)]
    fn standard_in(&self, value: u16, len: usize) -> Option<Vec<u8>> {
        let mut b = control_request(0x80, 6, value, 0, &[], len, 5);
        self.handle.ioctl(IOCTL_EP0, &mut b, 10_000).then(|| reply_data(&b))
    }
}

impl Transport for CyDevice {
    #[cfg(windows)]
    fn control_in(&mut self, request: u8, len: usize) -> Option<Vec<u8>> {
        let mut b = control_request(0xC0, request, 0, 0, &[], len, 10);
        self.handle.ioctl(IOCTL_EP0, &mut b, 10_000).then(|| reply_data(&b))
    }
    #[cfg(windows)]
    fn control_out(&mut self, request: u8, data: &[u8]) -> bool {
        let mut b = control_request(0x40, request, 0, 0, data, 0, 10);
        self.handle.ioctl(IOCTL_EP0, &mut b, 10_000)
    }
    #[cfg(windows)]
    fn bulk_in(&mut self, len: usize, timeout_ms: u32) -> Option<Vec<u8>> {
        let mut b = bulk_request(self.bulk_in, len);
        self.handle.ioctl(IOCTL_BULK, &mut b, timeout_ms).then(|| reply_data(&b))
    }
    #[cfg(not(windows))]
    fn control_in(&mut self, _: u8, _: usize) -> Option<Vec<u8>> {
        None
    }
    #[cfg(not(windows))]
    fn control_out(&mut self, _: u8, _: &[u8]) -> bool {
        false
    }
    #[cfg(not(windows))]
    fn bulk_in(&mut self, _: usize, _: u32) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_match_cyapi_layout() {
        let r = control_request(0xC0, 0xB1, 0, 0, &[], 20, 10);
        assert_eq!(r.len(), 0x26 + 20);
        assert_eq!(&r[..8], &[0xC0, 0xB1, 0, 0, 0, 0, 20, 0]);
        assert_eq!(&r[8..12], &10u32.to_le_bytes());
        assert_eq!(&r[0x1e..0x26], &[0x26, 0, 0, 0, 20, 0, 0, 0]);
        let w = control_request(0x40, 0xBA, 0, 0, &[7, 0], 0, 10);
        assert_eq!((w[6], &w[0x26..]), (2, &[7u8, 0][..]));
        let b = bulk_request(0x86, 4096);
        assert_eq!((b[0x0d], b.len()), (0x86, 0x26 + 4096));
        let mut done = b.clone();
        done[0x26] = 0xAA;
        done[0x22..0x26].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(reply_data(&done), vec![0xAA, 0]);
    }
}
