//! FTDI USB FIFO transport for the NanoPix 1 / 1.5 / 2 sensors, as
//! ConnUSBfifo.dll uses it: the device is opened by serial number, written
//! with FT_Write and drained with FT_GetQueueStatus + FT_Read (at most 64 KiB
//! per read), see PROGRESS.md.
//!
//! LumoDent ships no DLL. On Windows the FTDI driver package (the one the
//! sensor's own installer puts on the PC) installs `ftd2xx.dll` into System32
//! next to the ftdibus kernel driver; it is that driver's user-mode interface
//! and is loaded at run time here. Without it, no FTDI sensor is listed.

use std::time::{Duration, Instant};

pub const VENDOR_ID: u16 = 0x0403;

/// One FTDI device the driver lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub description: String,
    pub vendor: u16,
    pub product: u16,
}

#[cfg(windows)]
mod imp {
    use super::Device;
    use std::ffi::c_void;

    #[link(name = "kernel32", kind = "raw-dylib")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> isize;
        fn GetProcAddress(module: isize, name: *const u8) -> *const c_void;
    }

    type Handle = *mut c_void;
    type CreateList = unsafe extern "system" fn(*mut u32) -> u32;
    type GetList = unsafe extern "system" fn(*mut Node, *mut u32) -> u32;
    type OpenEx = unsafe extern "system" fn(*const c_void, u32, *mut Handle) -> u32;
    type Close = unsafe extern "system" fn(Handle) -> u32;
    type Rw = unsafe extern "system" fn(Handle, *mut c_void, u32, *mut u32) -> u32;
    type Queue = unsafe extern "system" fn(Handle, *mut u32) -> u32;
    type Timeouts = unsafe extern "system" fn(Handle, u32, u32) -> u32;
    type Purge = unsafe extern "system" fn(Handle, u32) -> u32;
    type UsbParams = unsafe extern "system" fn(Handle, u32, u32) -> u32;
    type Latency = unsafe extern "system" fn(Handle, u8) -> u32;

    /// FT_DEVICE_LIST_INFO_NODE.
    #[repr(C)]
    pub struct Node {
        flags: u32,
        kind: u32,
        id: u32,
        loc: u32,
        serial: [u8; 16],
        description: [u8; 64],
        handle: Handle,
    }

    struct Api {
        create_list: CreateList,
        get_list: GetList,
        open_ex: OpenEx,
        close: Close,
        read: Rw,
        write: Rw,
        queue: Queue,
        timeouts: Timeouts,
        purge: Purge,
        usb_params: UsbParams,
        latency: Latency,
    }

    fn api() -> Option<&'static Api> {
        static API: std::sync::OnceLock<Option<Api>> = std::sync::OnceLock::new();
        API.get_or_init(|| unsafe {
            let name: Vec<u16> = "ftd2xx.dll\0".encode_utf16().collect();
            let module = LoadLibraryW(name.as_ptr());
            if module == 0 {
                return None;
            }
            macro_rules! sym {
                ($n:literal) => {{
                    let p = GetProcAddress(module, concat!($n, "\0").as_ptr());
                    if p.is_null() {
                        return None;
                    }
                    std::mem::transmute::<*const c_void, _>(p)
                }};
            }
            Some(Api {
                create_list: sym!("FT_CreateDeviceInfoList"),
                get_list: sym!("FT_GetDeviceInfoList"),
                open_ex: sym!("FT_OpenEx"),
                close: sym!("FT_Close"),
                read: sym!("FT_Read"),
                write: sym!("FT_Write"),
                queue: sym!("FT_GetQueueStatus"),
                timeouts: sym!("FT_SetTimeouts"),
                purge: sym!("FT_Purge"),
                usb_params: sym!("FT_SetUSBParameters"),
                latency: sym!("FT_SetLatencyTimer"),
            })
        })
        .as_ref()
    }

    fn text(b: &[u8]) -> String {
        String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned()
    }

    pub fn list() -> Vec<Device> {
        let Some(api) = api() else { return Vec::new() };
        unsafe {
            let mut n = 0u32;
            if (api.create_list)(&mut n) != 0 || n == 0 {
                return Vec::new();
            }
            let mut nodes: Vec<Node> = (0..n)
                .map(|_| Node { flags: 0, kind: 0, id: 0, loc: 0, serial: [0; 16], description: [0; 64], handle: std::ptr::null_mut() })
                .collect();
            if (api.get_list)(nodes.as_mut_ptr(), &mut n) != 0 {
                return Vec::new();
            }
            nodes.truncate(n as usize);
            nodes
                .iter()
                // Bit 0 of Flags: opened by another program.
                .filter(|d| d.flags & 1 == 0)
                .map(|d| Device { serial: text(&d.serial), description: text(&d.description), vendor: (d.id >> 16) as u16, product: d.id as u16 })
                .collect()
        }
    }

    pub struct Port(Handle);
    // The handle is only used from the thread that owns the Port.
    unsafe impl Send for Port {}

    impl Port {
        pub fn open(serial: &str) -> Result<Self, String> {
            let api = api().ok_or("The FTDI driver (ftd2xx.dll) is not installed.")?;
            let arg = std::ffi::CString::new(serial).map_err(|e| e.to_string())?;
            let mut h: Handle = std::ptr::null_mut();
            unsafe {
                // FT_OPEN_BY_SERIAL_NUMBER
                let status = (api.open_ex)(arg.as_ptr().cast(), 1, &mut h);
                if status != 0 {
                    return Err(format!("FT_OpenEx failed ({status})"));
                }
                (api.usb_params)(h, 0x10000, 0x10000);
                (api.latency)(h, 2);
                (api.timeouts)(h, 100, 1000);
                (api.purge)(h, 3);
            }
            Ok(Port(h))
        }

        pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
            let api = api().ok_or("no driver")?;
            let mut done = 0u32;
            let s = unsafe { (api.write)(self.0, data.as_ptr() as *mut c_void, data.len() as u32, &mut done) };
            if s != 0 || done as usize != data.len() {
                return Err(format!("FT_Write failed ({s}, {done}/{})", data.len()));
            }
            Ok(())
        }

        /// What is queued now, at most 64 KiB, without blocking.
        pub fn read_available(&mut self, out: &mut Vec<u8>) -> Result<usize, String> {
            let api = api().ok_or("no driver")?;
            unsafe {
                let mut queued = 0u32;
                let s = (api.queue)(self.0, &mut queued);
                if s != 0 {
                    return Err(format!("FT_GetQueueStatus failed ({s})"));
                }
                let want = queued.min(0x10000);
                if want == 0 {
                    return Ok(0);
                }
                let start = out.len();
                out.resize(start + want as usize, 0);
                let mut got = 0u32;
                let s = (api.read)(self.0, out[start..].as_mut_ptr().cast(), want, &mut got);
                out.truncate(start + got as usize);
                if s != 0 {
                    return Err(format!("FT_Read failed ({s})"));
                }
                Ok(got as usize)
            }
        }
    }

    impl Drop for Port {
        fn drop(&mut self) {
            if let Some(api) = api() {
                unsafe { (api.close)(self.0) };
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Device;
    pub fn list() -> Vec<Device> {
        Vec::new()
    }
    pub struct Port;
    impl Port {
        pub fn open(_: &str) -> Result<Self, String> {
            Err("FTDI sensors are only supported on Windows for now.".into())
        }
        pub fn write(&mut self, _: &[u8]) -> Result<(), String> {
            Err("unsupported".into())
        }
        pub fn read_available(&mut self, _: &mut Vec<u8>) -> Result<usize, String> {
            Err("unsupported".into())
        }
    }
}

pub use imp::Port;

/// FTDI devices that are not open in another program.
pub fn list() -> Vec<Device> {
    imp::list().into_iter().filter(|d| d.vendor == VENDOR_ID).collect()
}

/// A byte pipe to the detector: the FTDI port, or a fake in tests.
pub trait Pipe {
    fn send(&mut self, data: &[u8]) -> Result<(), String>;
    /// Appends what has arrived; returns how many bytes.
    fn receive(&mut self, out: &mut Vec<u8>) -> Result<usize, String>;
}

impl Pipe for Port {
    fn send(&mut self, data: &[u8]) -> Result<(), String> {
        self.write(data)
    }
    fn receive(&mut self, out: &mut Vec<u8>) -> Result<usize, String> {
        self.read_available(out)
    }
}

/// Polls `pipe` until `done` returns true or `timeout` passes.
pub fn receive_until<P: Pipe + ?Sized>(pipe: &mut P, buf: &mut Vec<u8>, timeout: Duration, mut done: impl FnMut(&mut Vec<u8>) -> bool) -> Result<bool, String> {
    let end = Instant::now() + timeout;
    loop {
        if done(buf) {
            return Ok(true);
        }
        if Instant::now() >= end {
            return Ok(false);
        }
        if pipe.receive(buf)? == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
