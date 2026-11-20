//! NanoPix-E sensor protocol, ported from sensor_io.dll (HDR_MultiDevice).
//!
//! The DLL talks to a Cypress FX USB chip through CyAPI: vendor control
//! requests (request type 2, value/index 0) carry commands and short replies,
//! and one bulk IN endpoint carries line and frame data. Everything here goes
//! through [`Transport`], so the protocol runs against a fake device in tests;
//! a WinUSB transport for the real sensor still has to be written and tried
//! with the hardware.

/// Vendor request codes used by sensor_io.dll.
pub mod req {
    /// IN, 20 bytes: product, width, height, serial.
    pub const DEVICE_INFO: u8 = 0xB1;
    /// OUT then IN ack (0xB5): grab a few dark lines for the X-ray trigger.
    pub const READ_LINES: u8 = 0xB5;
    /// OUT: start reading the frame over bulk.
    pub const READ_FRAME: u8 = 0xB6;
    /// OUT: reset / re-arm the readout.
    pub const RESET: u8 = 0xB7;
    /// IN, 2 bytes: cable / pay status check.
    pub const STATUS: u8 = 0xB8;
    /// IN, 1 byte: sensor generation (2 = newer line format).
    pub const GENERATION: u8 = 0xB9;
    /// OUT, 1 byte: integration time.
    pub const INTEGRATION_TIME: u8 = 0xBA;
    /// OUT: power on the newer sensors.
    pub const POWER_ON: u8 = 0xBC;
    /// OUT: power off the newer sensors.
    pub const POWER_OFF: u8 = 0xBD;
    /// OUT: prepare a frame on the newer sensors.
    pub const PREPARE: u8 = 0xBE;
    /// OUT, 1 byte: binning.
    pub const BINNING: u8 = 0xC0;
    /// OUT: re-enumerate (power mode 1).
    pub const RENUMERATE: u8 = 0xC2;
    /// IN, 1 byte: power mode.
    pub const POWER_MODE: u8 = 0xC3;
    /// IN, 36 bytes: two 18-byte firmware strings.
    pub const FIRMWARE: u8 = 0xC4;
    /// IN, 1 byte: customer flag.
    pub const CUSTOMER: u8 = 0xC6;
    /// IN, 1 byte: image bits (0xFF means 12).
    pub const IMAGE_BITS: u8 = 0xC7;
    /// IN, 1 byte: pay status (0xFF also clears the bit depth).
    pub const PAY_STATUS: u8 = 0xC8;
    /// OUT, 1 byte: HDR value.
    pub const HDR_VALUE: u8 = 0xD1;
    /// IN, 2 bytes: HDR compensation value.
    pub const HDR_COMP: u8 = 0xD3;
    /// IN, 1 byte: HDR mode.
    pub const HDR_MODE: u8 = 0xD4;
}

/// Error codes stored as sensor_io's "last error".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorError {
    NotFound = -1,
    WriteFailed = -2,
    ReadFailed = -3,
    BulkFailed = -4,
    NoXray = -5,
    Rearmed = -7,
}

/// USB access the protocol needs. Control transfers are vendor requests to
/// the device with value and index 0.
pub trait Transport {
    fn control_in(&mut self, request: u8, len: usize) -> Option<Vec<u8>>;
    fn control_out(&mut self, request: u8, data: &[u8]) -> bool;
    fn bulk_in(&mut self, len: usize, timeout_ms: u32) -> Option<Vec<u8>>;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceInfo {
    pub product: u16,
    pub width: u16,
    pub height: u16,
    pub serial: String,
}

/// An opened sensor (the 0x9FC-byte object sensor_io allocates).
pub struct Sensor<T: Transport> {
    pub usb: T,
    pub info: DeviceInfo,
    pub firmware: [String; 2],
    pub generation: u8,
    pub power_mode: u8,
    pub customer: u8,
    pub image_bits: u8,
    /// Divider applied to raw values on the newer sensors (4 for 12-bit, 1 for 14-bit).
    pub bit_divider: u16,
    pub pay_status: u8,
    pub hdr_mode: u8,
    pub binning: u16,
    /// Dark level measured at open; the trigger fires above it.
    pub baseline: i32,
    pub level: i32,
    pub threshold: i32,
    pub armed: bool,
    pub last_error: Option<SensorError>,
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Bytes needed for `n`, rounded up to the next 1 KiB like the DLL does.
fn round_kib(n: usize) -> usize {
    n - (n & 0x3ff) + 0x400
}

/// Decodes one pixel of the newer line format (two bytes, inverted).
pub fn gen2_pixel(b0: u8, b1: u8) -> u32 {
    ((0x80 - (b0 & 0x7f) as u32) * 0x80).wrapping_sub((b1 >> 1) as u32) >> 2
}

/// Line stride and the offset of pixel data for the newer sensors' widths.
fn gen2_layout(width: u16) -> Option<(usize, usize)> {
    match width {
        0x4b0 => Some((0x4bb, 0x12)),
        0x57a | 0x9dc | 0x510 => Some((width as usize + 7, 0xc)),
        _ => None,
    }
}

/// Index of the 0xAA sync byte among the first five words.
fn sync(buf: &[u8]) -> usize {
    (0..5).find(|i| buf.get(i * 2) == Some(&0xAA)).unwrap_or(5)
}

impl<T: Transport> Sensor<T> {
    fn get(&mut self, request: u8, len: usize) -> Option<Vec<u8>> {
        let r = self.usb.control_in(request, len);
        if r.is_none() {
            self.last_error = Some(SensorError::ReadFailed);
        }
        r
    }

    fn get1(&mut self, request: u8) -> Option<u8> {
        self.get(request, 1).and_then(|v| v.first().copied())
    }

    /// Sends a command and reads its one-byte reply, as every setter does.
    fn command(&mut self, request: u8, data: &[u8]) -> bool {
        if !self.usb.control_out(request, data) {
            self.last_error = Some(SensorError::WriteFailed);
        }
        self.usb.control_in(request, 1).is_some()
    }

    /// Sensor_OpenDevice after the USB handle is open: reads the device
    /// description, firmware, bit depth and HDR mode, then measures the dark
    /// baseline. Returns None where the DLL returns NULL.
    pub fn open(usb: T) -> Option<Self> {
        let mut s = Sensor {
            usb,
            info: DeviceInfo::default(),
            firmware: Default::default(),
            generation: 0,
            power_mode: 0,
            customer: 0,
            image_bits: 0,
            bit_divider: 1,
            pay_status: 0,
            hdr_mode: 0,
            binning: 1,
            baseline: 0,
            level: 0,
            threshold: 0,
            armed: false,
            last_error: None,
        };
        s.read_info()?;
        s.reset();
        if let Some(g) = s.get1(req::GENERATION) {
            s.generation = g;
        }
        if let Some(fw) = s.get(req::FIRMWARE, 36) {
            let fw = [fw.get(..18).unwrap_or(&[]), fw.get(18..36).unwrap_or(&[])];
            s.firmware = [cstr(fw[0]), cstr(fw[1])];
        }
        if let Some(p) = s.get1(req::POWER_MODE) {
            s.power_mode = p;
        }
        if let Some(c) = s.get1(req::CUSTOMER) {
            s.customer = c;
        }
        if let Some(bits) = s.get1(req::IMAGE_BITS) {
            s.image_bits = if bits == 0xff { 12 } else { bits };
            match s.image_bits {
                12 => s.bit_divider = 4,
                14 => s.bit_divider = 1,
                _ => {}
            }
        }
        if s.generation == 2 {
            s.command(req::POWER_ON, &[0]);
        }
        if let Some(p) = s.get1(req::PAY_STATUS) {
            s.pay_status = p;
            if p == 0xff {
                s.image_bits = 0;
            }
        }
        if let Some(m) = s.get1(req::HDR_MODE) {
            s.hdr_mode = m;
        }
        let ok = if s.generation == 2 { s.measure_gen2(true).is_some() } else { s.measure_gen1(2).is_some() };
        s.armed = ok;
        if !ok && s.generation == 0 {
            return None;
        }
        Some(s)
    }

    /// 0xB1: product, width, height, then an 8-character serial. The product
    /// must be one of the sensor_io families and NanoPix-E (0x44) serials
    /// start with a digit.
    fn read_info(&mut self) -> Option<()> {
        let b = self.get(req::DEVICE_INFO, 20)?;
        if b.len() < 6 {
            return None;
        }
        let u = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        self.info = DeviceInfo { product: u(0), width: u(2), height: u(4), serial: cstr(b.get(6..15).unwrap_or(&[])) };
        if self.info.product == 0x44 && !self.info.serial.starts_with(|c: char| ('1'..='9').contains(&c)) {
            return None;
        }
        Some(())
    }

    pub fn reset(&mut self) {
        self.command(req::RESET, &[0]);
    }

    /// Dark-line read of the older sensors: a few lines over bulk, ack 0xB5,
    /// then the mean of the little-endian words. `tries` bulk reads are made
    /// (2 at open, 1 while waiting for X-rays).
    fn measure_gen1(&mut self, tries: usize) -> Option<i32> {
        let len = ((self.info.width as usize + 25) * 10) & !0x3ff;
        let mut buf = Vec::new();
        for _ in 0..tries {
            if !self.usb.control_out(req::READ_LINES, &[0]) {
                self.last_error = Some(SensorError::WriteFailed);
                return None;
            }
            match self.usb.bulk_in(len, 2000) {
                Some(b) => buf = b,
                None => {
                    self.last_error = Some(SensorError::BulkFailed);
                    return None;
                }
            }
        }
        if self.usb.control_in(req::READ_LINES, 1).and_then(|v| v.first().copied()) != Some(0xB5) || len == 0 {
            self.last_error = Some(SensorError::ReadFailed);
            return None;
        }
        let sum: i64 = buf.chunks_exact(2).take(len / 2).map(|w| u16::from_le_bytes([w[0], w[1]]) as i64).sum();
        let mean = (sum * 2 / len as i64) as i32;
        if tries > 1 {
            self.baseline = mean;
        }
        Some(mean)
    }

    /// Eight lines of the newer sensors, decoded; returns the mean and the
    /// count of bright (> 4000) pixels. At open the mean becomes the baseline.
    fn measure_gen2(&mut self, at_open: bool) -> Option<(i32, usize)> {
        let (stride, offset) = gen2_layout(self.info.width).unwrap_or((0, 0));
        let len = round_kib(stride * 0x12);
        if !self.usb.control_out(req::READ_LINES, &[0]) {
            self.last_error = Some(SensorError::WriteFailed);
            return None;
        }
        let Some(buf) = self.usb.bulk_in(len, 5000) else {
            self.last_error = Some(SensorError::BulkFailed);
            self.reset();
            return None;
        };
        if at_open {
            self.reset();
        }
        let s = sync(&buf);
        let w = self.info.width as usize;
        let (mut sum, mut bright) = (0i64, 0usize);
        for line in 0..8 {
            for x in 0..w {
                let i = ((line + 1) * stride + x + s) * 2 + offset;
                let (b0, b1) = (buf.get(i).copied().unwrap_or(0), buf.get(i + 1).copied().unwrap_or(0));
                let v = gen2_pixel(b0, b1);
                bright += (v > 4000) as usize;
                sum += v as i64;
            }
        }
        let mean = if w == 0 { 0 } else { (sum * 2 / (w as i64 * 16)) as i32 };
        if at_open {
            self.baseline = mean;
        }
        Some((mean, bright))
    }

    /// Sensor_GetXrayState: true once the line level rises above
    /// baseline + threshold (the exposure has started).
    pub fn xray_detected(&mut self) -> bool {
        self.last_error = Some(SensorError::NoXray);
        if !self.armed {
            self.armed = true;
            self.last_error = Some(SensorError::Rearmed);
            self.reset();
            let ok = if self.generation == 2 {
                self.read_info().is_some() && self.measure_gen2(true).is_some()
            } else {
                self.read_info().is_some() && self.measure_gen1(2).is_some()
            };
            if !ok {
                return false;
            }
        }
        if self.generation == 2 {
            let Some((level, bright)) = self.measure_gen2(false) else { return false };
            if bright > 100 {
                self.reset();
                return false;
            }
            self.level = level;
            if level <= self.threshold + self.baseline {
                self.reset();
                return false;
            }
            return true;
        }
        let Some(level) = self.measure_gen1(1) else { return false };
        self.level = level;
        level > self.threshold + self.baseline
    }

    /// Bytes Sensor_LoadRawImage writes (16-bit samples, or half with 2×2 binning).
    pub fn image_bytes(&self) -> usize {
        let n = self.info.width as usize * self.info.height as usize;
        if self.binning == 1 { n * 2 } else { n / 2 }
    }

    /// Sensor_LoadRawImage: reads the frame and returns big-endian 16-bit
    /// samples, `width / binning` by `height / binning`.
    pub fn load_raw_image(&mut self) -> Option<Vec<u8>> {
        if !self.armed {
            return None;
        }
        if self.generation == 2 {
            return self.load_gen2();
        }
        let (w, h) = (self.info.width as usize, self.info.height as usize);
        let len = round_kib((w + 25) * h * 2);
        if !self.usb.control_out(req::READ_FRAME, &[0]) {
            self.last_error = Some(SensorError::WriteFailed);
            return None;
        }
        let Some(buf) = self.usb.bulk_in(len, 30000) else {
            self.last_error = Some(SensorError::BulkFailed);
            return None;
        };
        let mut out = vec![0u8; w * h * 2];
        for y in 0..h {
            for x in 0..w {
                let i = ((w + 25) * y + x) * 2 + 0x2e;
                let (lo, hi) = (buf.get(i).copied().unwrap_or(0) as i32, buf.get(i + 1).copied().unwrap_or(0) as i32);
                let v = ((hi - 8) * 256 + lo).max(0);
                out[(y * w + x) * 2] = (v >> 8) as u8;
                out[(y * w + x) * 2 + 1] = v as u8;
            }
        }
        self.reset();
        Some(out)
    }

    fn load_gen2(&mut self) -> Option<Vec<u8>> {
        self.command(req::PREPARE, &[0]);
        let (w, h) = (self.info.width as usize, self.info.height as usize);
        let (stride, offset) = gen2_layout(self.info.width)?;
        let rows = if w == 0x4b0 { h + 4 } else { h + 2 };
        let len = round_kib(rows * stride * 2);
        if !self.usb.control_out(req::READ_FRAME, &[0]) {
            self.last_error = Some(SensorError::WriteFailed);
            return None;
        }
        let Some(buf) = self.usb.bulk_in(len, 30000) else {
            self.last_error = Some(SensorError::BulkFailed);
            self.reset();
            self.command(req::POWER_OFF, &[0]);
            self.command(req::POWER_ON, &[0]);
            self.measure_gen2(true);
            return None;
        };
        let s = sync(&buf);
        let bin = self.binning.max(1) as usize;
        let mut out = vec![0u8; w * h * 2 / (bin * bin).max(1) + 2];
        for y in 0..h / bin {
            for x in (0..w).step_by(bin) {
                let i = if w == 0x4b0 { ((w + 11) * (y + 3) + x + s) * 2 + 0x12 } else { (s + stride * (y + 1) + x) * 2 + offset };
                let (b0, b1) = (buf.get(i).copied().unwrap_or(0) as i32, buf.get(i + 1).copied().unwrap_or(0) as i32);
                let v = self.gen2_value(b0, b1);
                let at = (w * y + x) * 2 / bin;
                if at + 1 < out.len() {
                    out[at] = (v >> 8) as u8;
                    out[at + 1] = v as u8;
                }
            }
        }
        self.clear_chamfers(&mut out);
        self.reset();
        out.truncate(self.image_bytes());
        Some(out)
    }

    /// Raw value of one pixel of the newer frame format, by HDR and bit mode.
    fn gen2_value(&self, b0: i32, b1: i32) -> i32 {
        let div = self.bit_divider.max(1) as i32;
        if self.hdr_mode != 1 {
            return (0x3538 - b0 * 0x80 - (b1 >> 1)) / div;
        }
        let (hi, lsb) = (b1 >> 1, b1 & 1);
        let raw = 0x3fff - b0 * 0x80 - hi;
        if self.customer == 1 {
            let v = if self.image_bits == 16 { raw } else { raw / (div * 4) };
            lsb + (v / 2) * 2
        } else if self.image_bits == 16 {
            lsb + raw * 2
        } else {
            let v = raw / (div * 2);
            lsb + ((if v < 0 { v + 1 } else { v }) >> 1) * 2
        }
    }

    /// Zeroes the cut-off triangles at the bottom corners, like the DLL
    /// does for its three chamfered sizes.
    fn clear_chamfers(&self, out: &mut [u8]) {
        let (w, bin) = (self.info.width as usize, self.binning.max(1) as usize);
        let (top, size) = match w {
            0x57a => (0x6d4, 0x8c),
            0x9dc => (0xdac, 0x8c),
            0x510 => (0x614, 0xd8),
            _ => return,
        };
        let mut zero = |row: usize, x: usize| {
            let at = (row * w + x) * 2 / bin;
            if at + 1 < out.len() {
                out[at] = 0;
                out[at + 1] = 0;
            }
        };
        for k in 0..size / bin {
            let row = top / bin + k;
            for x in (0..bin * k).step_by(bin) {
                zero(row, x);
            }
            for x in (w - bin * k..w).step_by(bin) {
                zero(row, x);
            }
        }
    }

    pub fn set_integration_time(&mut self, value: u8) -> bool {
        self.command(req::INTEGRATION_TIME, &[value, 0])
    }

    /// Only the newer sensors bin; the older ones accept and ignore it.
    pub fn set_binning(&mut self, mode: u8) -> bool {
        if self.generation != 2 {
            return true;
        }
        let ok = self.command(req::BINNING, &[mode]);
        if ok {
            self.binning = if mode == 0 { 1 } else { mode as u16 };
        }
        ok
    }

    pub fn set_threshold(&mut self, level: i32) {
        self.threshold = level;
    }

    pub fn set_hdr_value(&mut self, value: u8) -> bool {
        self.command(req::HDR_VALUE, &[value])
    }

    /// 200 when the read fails, like the DLL.
    pub fn hdr_comp_value(&mut self) -> u16 {
        match self.get(req::HDR_COMP, 2) {
            Some(b) if b.len() >= 2 => u16::from_le_bytes([b[0], b[1]]),
            _ => 200,
        }
    }

    /// Sensor_CloseDevice: the newer sensors are powered off first.
    pub fn close(mut self) -> T {
        if self.generation == 2 {
            self.command(req::POWER_OFF, &[0]);
        }
        self.usb
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::collections::HashMap;

    /// A pretend sensor: canned control replies and a queue of bulk reads,
    /// recording every request.
    #[derive(Default)]
    pub struct FakeUsb {
        pub replies: HashMap<u8, Vec<u8>>,
        pub bulk: Vec<Vec<u8>>,
        pub log: Vec<(char, u8)>,
    }

    impl Transport for FakeUsb {
        fn control_in(&mut self, request: u8, len: usize) -> Option<Vec<u8>> {
            self.log.push(('i', request));
            let mut r = self.replies.get(&request).cloned().unwrap_or_else(|| vec![0]);
            r.resize(len, 0);
            Some(r)
        }
        fn control_out(&mut self, request: u8, _data: &[u8]) -> bool {
            self.log.push(('o', request));
            true
        }
        fn bulk_in(&mut self, len: usize, _timeout_ms: u32) -> Option<Vec<u8>> {
            self.log.push(('b', 0));
            let mut b = if self.bulk.is_empty() { None } else { Some(self.bulk.remove(0)) }?;
            b.resize(len, 0);
            Some(b)
        }
    }

    pub fn info(product: u16, w: u16, h: u16, serial: &str) -> Vec<u8> {
        let mut b = Vec::new();
        for v in [product, w, h] {
            b.extend(v.to_le_bytes());
        }
        b.extend(serial.as_bytes());
        b.resize(20, 0);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    /// Older sensor frame: pixel (x, y) stored little-endian with a +8 high-byte bias.
    fn gen1_frame(w: usize, h: usize, f: impl Fn(usize, usize) -> u16) -> Vec<u8> {
        let mut b = vec![0u8; round_kib((w + 25) * h * 2)];
        for y in 0..h {
            for x in 0..w {
                let v = f(x, y) as i32 + 8 * 256;
                let i = ((w + 25) * y + x) * 2 + 0x2e;
                b[i] = v as u8;
                b[i + 1] = (v >> 8) as u8;
            }
        }
        b
    }

    fn dark(w: usize, level: u16) -> Vec<u8> {
        let len = ((w + 25) * 10) & !0x3ff;
        (0..len / 2).flat_map(|_| level.to_le_bytes()).collect()
    }

    fn gen1(w: u16, h: u16) -> FakeUsb {
        let mut usb = FakeUsb::default();
        usb.replies.insert(req::DEVICE_INFO, info(0x44, w, h, "12090806"));
        usb.replies.insert(req::FIRMWARE, b"FW 1.2\0\0\0\0\0\0\0\0\0\0\0\0FPGA 3.4".to_vec());
        usb.replies.insert(req::IMAGE_BITS, vec![14]);
        usb.replies.insert(req::READ_LINES, vec![0xB5]);
        usb.bulk = vec![dark(w as usize, 300), dark(w as usize, 300)];
        usb
    }

    #[test]
    fn opens_reads_description_and_baseline() {
        let s = Sensor::open(gen1(120, 80)).expect("open");
        assert_eq!(s.info, DeviceInfo { product: 0x44, width: 120, height: 80, serial: "12090806".into() });
        assert_eq!(s.firmware, ["FW 1.2".to_string(), "FPGA 3.4".to_string()]);
        assert_eq!((s.image_bits, s.bit_divider), (14, 1));
        assert_eq!(s.baseline, 300);
        assert!(s.armed);
        assert_eq!(s.image_bytes(), 120 * 80 * 2);
    }

    #[test]
    fn rejects_e_series_serial_without_leading_digit() {
        let mut usb = gen1(120, 80);
        usb.replies.insert(req::DEVICE_INFO, info(0x44, 120, 80, "X1234567"));
        assert!(Sensor::open(usb).is_none());
    }

    #[test]
    fn xray_trigger_and_frame_readout() {
        let mut s = Sensor::open(gen1(120, 80)).unwrap();
        s.set_threshold(50);
        s.usb.bulk.push(dark(120, 320));
        assert!(!s.xray_detected(), "320 is under 300 + 50");
        s.usb.bulk.push(dark(120, 900));
        assert!(s.xray_detected());
        assert_eq!(s.level, 900);
        s.usb.bulk.push(gen1_frame(120, 80, |x, y| (x * 7 + y * 3) as u16));
        let img = s.load_raw_image().unwrap();
        let px = |x: usize, y: usize| u16::from_be_bytes([img[(y * 120 + x) * 2], img[(y * 120 + x) * 2 + 1]]);
        assert_eq!(px(0, 0), 0);
        assert_eq!(px(10, 5), 85);
        assert_eq!(px(119, 79), 119 * 7 + 79 * 3);
        assert_eq!(s.usb.log.last(), Some(&('i', req::RESET)), "the frame read ends with a reset");
    }

    #[test]
    fn gen2_pixel_decoding() {
        // b0 = 0x7f, b1 = 0xfe gives the darkest value, b0 = 0 the brightest.
        assert_eq!(gen2_pixel(0x7f, 0xfe), 0);
        assert_eq!(gen2_pixel(0, 0), 0x80 * 0x80 >> 2);
        assert_eq!(sync(&[0, 0, 0, 0, 0xAA]), 2);
    }

    #[test]
    fn gen2_frame_with_chamfers() {
        let (w, h) = (0x510usize, 0x614 + 0xd8);
        let mut usb = FakeUsb::default();
        usb.replies.insert(req::DEVICE_INFO, info(0x44, w as u16, h as u16, "12090806"));
        usb.replies.insert(req::GENERATION, vec![2]);
        usb.replies.insert(req::IMAGE_BITS, vec![14]);
        let (stride, offset) = gen2_layout(w as u16).unwrap();
        usb.bulk.push(vec![0x7f; round_kib(stride * 0x12)]);
        let mut s = Sensor::open(usb).unwrap();
        assert_eq!(s.generation, 2);
        let mut frame = vec![0u8; round_kib((h + 2) * stride * 2)];
        for y in 0..h {
            for x in 0..w {
                let i = (stride * (y + 1) + x + 5) * 2 + offset;
                frame[i] = 0x40;
                frame[i + 1] = 0x10;
            }
        }
        s.usb.bulk.push(frame);
        let img = s.load_raw_image().unwrap();
        assert_eq!(img.len(), w * h * 2);
        let px = |x: usize, y: usize| u16::from_be_bytes([img[(y * w + x) * 2], img[(y * w + x) * 2 + 1]]);
        assert_eq!(px(600, 800) as i32, 0x3538 - 0x40 * 0x80 - 8);
        // Bottom-left chamfer: row 0x614 + 100 has its first 100 pixels cleared.
        assert_eq!(px(50, 0x614 + 100), 0);
        assert_ne!(px(150, 0x614 + 100), 0);
        assert_eq!(px(w - 1, 0x614 + 100), 0);
        assert!(s.usb.log.contains(&('o', req::PREPARE)));
        let usb = s.close();
        assert_eq!(usb.log.iter().filter(|e| **e == ('o', req::POWER_OFF)).count(), 1);
    }
}
