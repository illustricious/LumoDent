//! A pretend NanoPix-E sensor that answers the sensor_io protocol the way the
//! hardware does (as far as the DLL shows), so the whole capture path runs
//! without the device: open, dark baseline, waiting for X-rays, the frame
//! readout and decoding. "Exposing" it makes the next line read bright and
//! the frame after that carry a synthetic tooth phantom.

use crate::usb_sensor::{req, Transport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const WIDTH: u16 = 1200;
pub const HEIGHT: u16 = 1600;
/// NanoPix-E serials must start with 1-9 (sensor_io checks it).
pub const SERIAL: &str = "99000001";

/// Line stride and data offset of the 1200-wide newer format.
const STRIDE: usize = 0x4bb;
const OFFSET: usize = 0x12;

pub struct Simulator {
    /// Set to fire an exposure; cleared once the frame has been read.
    pub exposure: Arc<AtomicBool>,
    last_out: u8,
    triggered: bool,
    seed: u32,
}

impl Simulator {
    pub fn new() -> (Self, Arc<AtomicBool>) {
        let exposure = Arc::new(AtomicBool::new(false));
        (Simulator { exposure: exposure.clone(), last_out: 0, triggered: false, seed: 12345 }, exposure)
    }

    fn noise(&mut self) -> i32 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        ((self.seed >> 16) % 200) as i32 - 100
    }

    /// Two bytes of the newer format for a decoded value (14-bit, HDR off):
    /// value = 0x3538 - b0·128 - (b1 >> 1).
    pub fn encode(value: i32) -> (u8, u8) {
        let raw = (0x3538 - value.clamp(0, 0x3538)) as u32;
        (((raw >> 7) & 0x7f) as u8, ((raw & 0x7f) << 1) as u8)
    }

    fn lines(&self, len: usize, bright: bool) -> Vec<u8> {
        let mut b = vec![0u8; len];
        b[0] = 0xAA;
        let (b0, b1) = if bright { (0x20, 0x00) } else { (0x7f, 0xfe) };
        for line in 0..8 {
            for x in 0..WIDTH as usize {
                let i = ((line + 1) * STRIDE + x) * 2 + OFFSET;
                if i + 1 < len {
                    b[i] = b0;
                    b[i + 1] = b1;
                }
            }
        }
        b
    }

    /// Dose-like counts of a phantom: open field high, two teeth low.
    pub fn phantom(x: usize, y: usize) -> i32 {
        let (w, h) = (WIDTH as f32, HEIGHT as f32);
        let (fx, fy) = (x as f32 / w, y as f32 / h);
        let tooth = |cx: f32| {
            let dx = (fx - cx) / 0.17;
            // Crown: an ellipse at the top; root: a tapering band below it.
            let crown = dx * dx + ((fy - 0.33) / 0.2).powi(2) < 1.0;
            let root = fy > 0.33 && fy < 0.9 && dx.abs() < 0.55 * (1.0 - (fy - 0.33) / 0.65);
            let enamel = crown && dx * dx + ((fy - 0.33) / 0.2).powi(2) > 0.7;
            let canal = root && dx.abs() < 0.08;
            (crown || root, enamel, canal)
        };
        let mut v = 11000.0 - 3000.0 * fy;
        for cx in [0.3, 0.7] {
            let (inside, enamel, canal) = tooth(cx);
            if inside {
                v = if canal { 6500.0 } else if enamel { 1500.0 } else { 3500.0 };
            }
        }
        if fy > 0.55 && v > 6000.0 {
            v = 7000.0; // bone around the roots
        }
        v as i32
    }

    fn frame(&mut self, len: usize) -> Vec<u8> {
        let mut b = vec![0u8; len];
        b[0] = 0xAA;
        for y in 0..HEIGHT as usize {
            for x in 0..WIDTH as usize {
                let i = ((WIDTH as usize + 11) * (y + 3) + x) * 2 + OFFSET;
                let (b0, b1) = Self::encode(Self::phantom(x, y) + self.noise());
                if i + 1 < len {
                    b[i] = b0;
                    b[i + 1] = b1;
                }
            }
        }
        b
    }
}

impl Transport for Simulator {
    fn control_in(&mut self, request: u8, len: usize) -> Option<Vec<u8>> {
        let mut r = match request {
            req::DEVICE_INFO => {
                let mut b = Vec::new();
                for v in [0x44u16, WIDTH, HEIGHT] {
                    b.extend(v.to_le_bytes());
                }
                b.extend(SERIAL.as_bytes());
                b
            }
            req::GENERATION => vec![2],
            req::FIRMWARE => {
                let mut b = b"SIM FW 1.0".to_vec();
                b.resize(18, 0);
                b.extend(b"SIM FPGA 1.0");
                b
            }
            req::IMAGE_BITS => vec![14],
            req::READ_LINES => vec![0xB5],
            req::HDR_COMP => 200u16.to_le_bytes().to_vec(),
            _ => vec![0],
        };
        r.resize(len, 0);
        Some(r)
    }

    fn control_out(&mut self, request: u8, _data: &[u8]) -> bool {
        self.last_out = request;
        true
    }

    fn bulk_in(&mut self, len: usize, _timeout_ms: u32) -> Option<Vec<u8>> {
        match self.last_out {
            req::READ_LINES => {
                let bright = self.exposure.load(Ordering::SeqCst);
                self.triggered |= bright;
                Some(self.lines(len, bright))
            }
            req::READ_FRAME => {
                let f = self.frame(len);
                if self.triggered {
                    self.triggered = false;
                    self.exposure.store(false, Ordering::SeqCst);
                }
                Some(f)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_sensor::Sensor;

    #[test]
    fn full_capture_through_the_protocol() {
        let (sim, exposure) = Simulator::new();
        let mut s = Sensor::open(sim).expect("open");
        assert_eq!((s.info.width, s.info.height, s.generation, s.info.serial.as_str()), (WIDTH, HEIGHT, 2, SERIAL));
        assert!(!s.xray_detected(), "no exposure yet");
        exposure.store(true, Ordering::SeqCst);
        assert!(s.xray_detected());
        let img = s.load_raw_image().unwrap();
        assert!(!exposure.load(Ordering::SeqCst), "the exposure is consumed");
        let px = |x: usize, y: usize| u16::from_be_bytes([img[(y * WIDTH as usize + x) * 2], img[(y * WIDTH as usize + x) * 2 + 1]]) as i32;
        // Open field (top-left corner) is brighter in counts than enamel and dentine.
        let air = px(20, 20);
        let tooth = px(420, 560);
        assert!((air - Simulator::phantom(20, 20)).abs() <= 101, "air {air}");
        assert!(air > tooth + 5000, "air {air} tooth {tooth}");
        assert!(!s.xray_detected(), "back to waiting");
    }

    #[test]
    fn encoding_round_trips() {
        for v in [0, 1, 1500, 7000, 0x3538] {
            let (b0, b1) = Simulator::encode(v);
            assert_eq!(0x3538 - b0 as i32 * 0x80 - (b1 >> 1) as i32, v);
        }
    }
}
