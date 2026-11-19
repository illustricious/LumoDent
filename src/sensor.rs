//! Post-processing of raw intraoral sensor frames, the image half of the
//! reference `CFinnoRay` class (IOSensorAcquisition.dll). Frames are 16-bit
//! counts held as f32, the reference `imgmat` layout. Talking to the detector
//! (InitializeByPath, Connect, WaitForImage) goes through the vendor SDK and is
//! not part of this module.

/// Raw count range of the detector.
pub const FULL_SCALE: f32 = 65535.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Frame {
    pub fn new(width: usize, height: usize, data: Vec<f32>) -> Self {
        assert_eq!(data.len(), width * height);
        Frame { width, height, data }
    }
}

fn histogram<'a>(values: impl Iterator<Item = &'a f32>) -> Vec<u32> {
    let mut h = vec![0u32; 1 << 16];
    for &v in values {
        let b = v as i64;
        if (0..1 << 16).contains(&b) {
            h[b as usize] += 1;
        }
    }
    h
}

/// CalcGainMat (0x10007740): from a flat-field exposure, gain = mean / pixel,
/// with pixels below 0.001 treated as 0.001.
pub fn gain_map(flat: &Frame) -> Frame {
    let n = flat.data.len().max(1) as f64;
    let mean = (flat.data.iter().map(|&v| v as f64).sum::<f64>() / n) as f32;
    Frame { width: flat.width, height: flat.height, data: flat.data.iter().map(|&v| mean / v.max(0.001)).collect() }
}

/// Mean count of the brightest pixels: walks the histogram down from 65535
/// until more than 5000 pixels are covered (FUN_100255c0).
pub fn bright_level(frame: &Frame) -> f64 {
    let h = histogram(frame.data.iter());
    let (mut count, mut sum) = (0u64, 0f64);
    for v in (0..h.len()).rev() {
        count += h[v] as u64;
        sum += h[v] as f64 * v as f64;
        if count > 4999 {
            break;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

/// DoGainCorrection (0x10007d00 → FUN_100255c0): with one calibration the
/// frame is multiplied by its gain map. With several (the reference keeps four,
/// one per dose level), the map whose recorded level is closest to the frame's
/// bright level is used. Returns the index of the map applied.
pub fn gain_correct(frame: &mut Frame, maps: &[Frame], levels: &[i32]) -> Option<usize> {
    let pick = match maps.len() {
        0 => return None,
        1 => 0,
        _ => {
            let level = bright_level(frame);
            (0..levels.len().min(maps.len()))
                .min_by(|&a, &b| (level - levels[a] as f64).powi(2).total_cmp(&(level - levels[b] as f64).powi(2)))?
        }
    };
    let map = &maps[pick];
    if map.data.len() != frame.data.len() {
        return None;
    }
    for (v, g) in frame.data.iter_mut().zip(&map.data) {
        *v *= g;
    }
    Some(pick)
}

/// The defect mask AcqImageEx builds after correction (0x10006c30): a
/// histogram of the interior (excluding `border` pixels on each side), a high
/// cut where more than 1000 pixels lie above it (never below 20001), a low cut
/// where more than 1000 lie below it (never above 199). Pixels outside
/// [low, high] are marked true.
pub fn defect_mask(frame: &Frame, border: usize) -> Vec<bool> {
    let (w, h) = (frame.width, frame.height);
    let inside = |i: usize| {
        let (x, y) = (i % w, i / w);
        x >= border && y >= border && x + border < w && y + border < h
    };
    let hist = histogram(frame.data.iter().enumerate().filter(|(i, _)| inside(*i)).map(|(_, v)| v));
    let mut high = FULL_SCALE;
    let mut acc = 0u32;
    for v in (0..hist.len()).rev() {
        acc += hist[v];
        if acc > 1000 || v < 20001 {
            high = v as f32;
            break;
        }
    }
    let mut low = 0.0;
    acc = 0;
    for (v, &c) in hist.iter().enumerate() {
        acc += c;
        if acc > 1000 || v > 199 {
            low = v as f32;
            break;
        }
    }
    frame.data.iter().enumerate().map(|(i, &v)| inside(i) && (v < low || v > high)).collect()
}

/// Replaces masked pixels with the mean of their unmasked 3×3 neighbours. The
/// reference calls ImgFix from a separate correction library, not yet mapped;
/// this is LumoDent's own stand-in.
pub fn fix_pixels(frame: &mut Frame, mask: &[bool]) {
    let (w, h) = (frame.width as i64, frame.height as i64);
    let src = frame.data.clone();
    for i in (0..src.len()).filter(|&i| mask[i]) {
        let (x, y) = (i as i64 % w, i as i64 / w);
        let (mut sum, mut n) = (0f32, 0);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if (0..w).contains(&nx) && (0..h).contains(&ny) {
                    let j = (ny * w + nx) as usize;
                    if !mask[j] {
                        sum += src[j];
                        n += 1;
                    }
                }
            }
        }
        if n > 0 {
            frame.data[i] = sum / n as f32;
        }
    }
}

/// 16-bit counts to an 8-bit greyscale image, full-scale.
pub fn to_image(frame: &Frame) -> crate::image::Image {
    let pixels = frame.data.iter().map(|&v| ((v.clamp(0.0, FULL_SCALE) / FULL_SCALE * 255.0).round() as u32) * 0x010101).collect();
    crate::image::Image { width: frame.width, height: frame.height, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_map_flattens_a_vignetted_field() {
        let flat = Frame::new(2, 2, vec![100.0, 200.0, 300.0, 400.0]);
        let map = gain_map(&flat);
        let mut f = flat.clone();
        assert_eq!(gain_correct(&mut f, &[map], &[]), Some(0));
        assert!(f.data.iter().all(|v| (v - 250.0).abs() < 1e-3));
        // Dead pixels are clamped instead of dividing by zero.
        assert!(gain_map(&Frame::new(1, 1, vec![0.0])).data[0].is_finite());
    }

    #[test]
    fn nearest_dose_level_picks_the_map() {
        let frame = Frame::new(100, 100, vec![30000.0; 10000]);
        assert!((bright_level(&frame) - 30000.0).abs() < 1e-6);
        let ones = Frame::new(100, 100, vec![1.0; 10000]);
        let twos = Frame::new(100, 100, vec![2.0; 10000]);
        let mut f = frame.clone();
        assert_eq!(gain_correct(&mut f, &[ones, twos], &[10000, 32000]), Some(1));
        assert_eq!(f.data[0], 60000.0);
    }

    #[test]
    fn hot_and_dead_pixels_are_masked_and_fixed() {
        let mut data = vec![30000.0; 100 * 100];
        data[5050] = 65000.0;
        data[2020] = 0.0;
        let mut f = Frame::new(100, 100, data);
        let mask = defect_mask(&f, 2);
        assert!(mask[5050] && mask[2020]);
        assert_eq!(mask.iter().filter(|m| **m).count(), 2);
        fix_pixels(&mut f, &mask);
        assert_eq!((f.data[5050], f.data[2020]), (30000.0, 30000.0));
        assert_eq!(to_image(&f).pixels[0], 117 * 0x010101);
    }
}
