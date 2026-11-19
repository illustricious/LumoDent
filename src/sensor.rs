//! Post-processing of raw intraoral sensor frames, the image half of the
//! reference `CFinnoRay` class (IOSensorAcquisition.dll). Frames are 16-bit
//! counts held as f32, the reference `imgmat` layout. Talking to the detector
//! (InitializeByPath, Connect, WaitForImage, the trigger and delay registers)
//! goes through the vendor SDK over USB and is not part of this module.

use std::path::{Path, PathBuf};

/// Raw count range of the detector.
pub const FULL_SCALE: f32 = 65535.0;

/// The sensors the reference knows (table built at 0x10004f00). `code` is what
/// JudgeSensorSize returns after matching the detector's reported width and
/// height (attributes 0x138f and 0x1390); `corner` is the chamfer cut off the
/// top corners when the defect histogram is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensorModel {
    pub code: i32,
    pub name: &'static str,
    pub width: usize,
    pub height: usize,
    pub corner: usize,
}

pub const MODELS: [SensorModel; 3] = [
    SensorModel { code: 1000, name: "NanoPix 1", width: 1000, height: 1500, corner: 211 },
    SensorModel { code: 1500, name: "NanoPix 1.5", width: 1200, height: 1650, corner: 251 },
    SensorModel { code: 2000, name: "NanoPix 2", width: 1300, height: 1800, corner: 281 },
];

/// JudgeSensorSize (0x10008bf0): the model with exactly this frame size.
pub fn model_for_size(width: usize, height: usize) -> Option<SensorModel> {
    MODELS.iter().copied().find(|m| m.width == width && m.height == height)
}

/// The model for a JudgeSensorSize code (FUN_10008730 / FUN_10008ad0).
pub fn model(code: i32) -> Option<SensorModel> {
    MODELS.iter().copied().find(|m| m.code == code)
}

/// Trigger modes the detector accepts (SetTriggerMode 0x10009270 rejects
/// anything else with "triggermode must b type 6 and 7").
pub fn valid_trigger_mode(mode: i32) -> bool {
    mode == 6 || mode == 7
}

/// AcqImageEx error codes (0x10006c30), for messages.
pub fn acquisition_error(code: i32) -> &'static str {
    match code {
        0 => "OK",
        -1 => "Sensor size is not right, or end sign and progress were not reset",
        -2 => "Please connect the sensor",
        -3 => "No calibration file",
        -4 => "Setting the delay time failed",
        -5 => "Setting the trigger mode failed",
        -6 => "Setting the trigger threshold failed",
        -7 => "Writing the detector user RAM failed",
        -8 => "The detector reported an error while getting an image",
        -9 => "Waiting was stopped",
        -10 => "Post-processing failed",
        _ => "Unknown acquisition error",
    }
}

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

// ---- calibration files ---------------------------------------------------

/// Calibration files carry a 0x410-byte header before the pixels.
pub const CALIBRATION_HEADER: usize = 0x410;

/// One flat-field gain calibration, as GetAndCaliFile_Gain loads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Gain {
    /// The dose level: the first raw pixel of the flat field (ReadCaliFile_Gain 0x10008ce0).
    pub level: i32,
    pub map: Frame,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Calibration {
    pub gains: Vec<Gain>,
    /// From the `.dft` file: non-zero marks a defective pixel.
    pub defects: Option<Frame>,
}

/// The reference looks for its files in `<sensor folder>/Correct/Default`
/// and takes the first file with the given extension ("gn" or "dft").
pub fn find_calibration_file(sensor_dir: &Path, extension: &str) -> Option<PathBuf> {
    let dir = sensor_dir.join("Correct").join("Default");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(extension)))
        .collect();
    files.sort();
    files.into_iter().next()
}

/// GetAndCaliFile_Gain (0x10007d30): a `.gn` file holds one flat field, or
/// four (one per dose level) when it is larger than width × height × 4 bytes.
/// Each is width × height 16-bit little-endian counts after the header.
pub fn parse_gain_file(bytes: &[u8], model: SensorModel) -> Result<Vec<Gain>, String> {
    let n = model.width * model.height;
    let count = if bytes.len() > n * 4 { 4 } else { 1 };
    (0..count)
        .map(|k| {
            let start = CALIBRATION_HEADER + k * n * 2;
            let raw = bytes.get(start..start + n * 2).ok_or("The gain calibration file is too short.")?;
            let flat: Vec<f32> = raw.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]) as f32).collect();
            let level = flat[0] as i32;
            Ok(Gain { level, map: gain_map(&Frame::new(model.width, model.height, flat)) })
        })
        .collect()
}

/// GetBadPixelInfo (0x10008150): a `.dft` file is width × height bytes after
/// the header, non-zero where a pixel is defective.
pub fn parse_defect_file(bytes: &[u8], model: SensorModel) -> Result<Frame, String> {
    let n = model.width * model.height;
    let raw = bytes.get(CALIBRATION_HEADER..CALIBRATION_HEADER + n).ok_or("The defect file is too short.")?;
    Ok(Frame::new(model.width, model.height, raw.iter().map(|&b| b as f32).collect()))
}

/// Loads both files from `<sensor folder>/Correct/Default`. Like the
/// reference, a missing gain file is an error ("No CaliFile!") and a missing
/// defect file only skips pixel repair.
pub fn load_calibration(sensor_dir: &Path, model: SensorModel) -> Result<Calibration, String> {
    let gn = find_calibration_file(sensor_dir, "gn").ok_or("No gain calibration file.")?;
    let gains = parse_gain_file(&std::fs::read(&gn).map_err(|e| e.to_string())?, model)?;
    let defects = match find_calibration_file(sensor_dir, "dft") {
        Some(p) => Some(parse_defect_file(&std::fs::read(&p).map_err(|e| e.to_string())?, model)?),
        None => None,
    };
    Ok(Calibration { gains, defects })
}

// ---- correction ----------------------------------------------------------

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
/// frame is multiplied by its gain map. With four, the one whose level is
/// closest to the frame's bright level is used. Returns the index applied.
pub fn gain_correct(frame: &mut Frame, gains: &[Gain]) -> Option<usize> {
    let pick = match gains.len() {
        0 => return None,
        1 => 0,
        _ => {
            let level = bright_level(frame);
            (0..gains.len()).min_by(|&a, &b| (level - gains[a].level as f64).powi(2).total_cmp(&(level - gains[b].level as f64).powi(2)))?
        }
    };
    let map = &gains[pick].map;
    if map.data.len() != frame.data.len() {
        return None;
    }
    for (v, g) in frame.data.iter_mut().zip(&map.data) {
        *v *= g;
    }
    Some(pick)
}

/// ImgFix (0x10021220), called as ImgFix(img, img, defects, w, h, 1, 6, 200).
/// Iterates up to `passes` times; in each pass, every defective pixel with at
/// least one healthy 4-neighbour becomes the mean of the healthy pixels in its
/// (2·radius+1)² window, then all pixels fixed in that pass count as healthy.
/// Pixels with no healthy neighbour wait for a later pass, so large clusters
/// fill in from their edges.
pub fn fix_defects(frame: &mut Frame, defects: &Frame, radius: usize, passes: usize) {
    let (w, h) = (frame.width, frame.height);
    if defects.data.len() != w * h || w == 0 || h == 0 {
        return;
    }
    let mut bad: Vec<bool> = defects.data.iter().map(|v| v.abs() >= 1e-6).collect();
    let mut todo: Vec<(usize, usize)> = (0..w * h).filter(|&i| bad[i]).map(|i| (i % w, i / w)).collect();
    let mut pass = 0;
    while !todo.is_empty() && pass < passes.max(1) {
        pass += 1;
        let (mut keep, mut fixed) = (Vec::new(), Vec::new());
        for &(x, y) in &todo {
            let neighbours = [(x, y.saturating_sub(1)), (x, (y + 1).min(h - 1)), (x.saturating_sub(1), y), ((x + 1).min(w - 1), y)];
            if neighbours.iter().all(|&(nx, ny)| bad[ny * w + nx]) {
                keep.push((x, y));
                continue;
            }
            let (mut sum, mut n) = (0f32, 0);
            for yy in y.saturating_sub(radius)..=(y + radius).min(h - 1) {
                for xx in x.saturating_sub(radius)..=(x + radius).min(w - 1) {
                    if !bad[yy * w + xx] {
                        sum += frame.data[yy * w + xx];
                        n += 1;
                    }
                }
            }
            if n < 1 || sum <= 0.0 {
                keep.push((x, y));
            } else {
                frame.data[y * w + x] = sum / n as f32;
                fixed.push((x, y));
            }
        }
        for (x, y) in fixed {
            bad[y * w + x] = false;
        }
        todo = keep;
    }
}

/// The histogram mask AcqImageEx builds after correction (0x10006c30). Only
/// pixels below the two top chamfers are counted (x + y > corner and
/// x − y < width − corner); the high cut is where more than 1000 pixels lie
/// above it (never below 20001), the low cut where more than 1000 lie below it
/// (never above 199). The reference computes this into a scratch buffer and
/// discards it; it is kept here as a diagnostic for over- and under-exposure.
pub fn exposure_mask(frame: &Frame, corner: usize) -> Vec<bool> {
    let w = frame.width;
    let inside = |i: usize| {
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        x + y > corner as i64 && x - y < w as i64 - corner as i64
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

/// The post-processing tail of AcqImageEx: gain correction, then defect repair.
pub fn post_process(raw: &Frame, calibration: &Calibration) -> Result<Frame, String> {
    if raw.data.is_empty() {
        return Err(acquisition_error(-10).into());
    }
    let mut frame = raw.clone();
    if !calibration.gains.is_empty() && gain_correct(&mut frame, &calibration.gains).is_none() {
        return Err("The gain calibration does not match the sensor size.".into());
    }
    if let Some(defects) = &calibration.defects {
        fix_defects(&mut frame, defects, 1, 200);
    }
    Ok(frame)
}

/// Reads a headerless 16-bit little-endian raw frame of a known model.
pub fn parse_raw_frame(bytes: &[u8], model: SensorModel) -> Result<Frame, String> {
    let n = model.width * model.height;
    let raw = bytes.get(..n * 2).ok_or_else(|| format!("A {} frame needs {} bytes.", model.name, n * 2))?;
    Ok(Frame::new(model.width, model.height, raw.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]) as f32).collect()))
}

/// 16-bit counts to an 8-bit greyscale image, full-scale.
pub fn to_image(frame: &Frame) -> crate::image::Image {
    let pixels = frame.data.iter().map(|&v| ((v.clamp(0.0, FULL_SCALE) / FULL_SCALE * 255.0).round() as u32) * 0x010101).collect();
    crate::image::Image { width: frame.width, height: frame.height, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain(level: i32, value: f32, w: usize, h: usize) -> Gain {
        Gain { level, map: Frame::new(w, h, vec![value; w * h]) }
    }

    #[test]
    fn models_match_the_reference_table() {
        assert_eq!(model_for_size(1300, 1800).map(|m| m.name), Some("NanoPix 2"));
        assert_eq!(model(1500).map(|m| (m.width, m.height, m.corner)), Some((1200, 1650, 251)));
        assert!(model_for_size(1000, 1000).is_none());
        assert!(valid_trigger_mode(6) && valid_trigger_mode(7) && !valid_trigger_mode(5));
    }

    #[test]
    fn gain_map_flattens_a_vignetted_field() {
        let flat = Frame::new(2, 2, vec![100.0, 200.0, 300.0, 400.0]);
        let mut f = flat.clone();
        assert_eq!(gain_correct(&mut f, &[Gain { level: 100, map: gain_map(&flat) }]), Some(0));
        assert!(f.data.iter().all(|v| (v - 250.0).abs() < 1e-3));
        assert!(gain_map(&Frame::new(1, 1, vec![0.0])).data[0].is_finite());
    }

    #[test]
    fn nearest_dose_level_picks_the_map() {
        let frame = Frame::new(100, 100, vec![30000.0; 10000]);
        assert!((bright_level(&frame) - 30000.0).abs() < 1e-6);
        let mut f = frame.clone();
        assert_eq!(gain_correct(&mut f, &[gain(10000, 1.0, 100, 100), gain(32000, 2.0, 100, 100)]), Some(1));
        assert_eq!(f.data[0], 60000.0);
    }

    #[test]
    fn defects_fill_in_from_the_edges_of_a_cluster() {
        let (w, h) = (7, 7);
        let mut f = Frame::new(w, h, vec![100.0; w * h]);
        let mut d = vec![0.0; w * h];
        // A 3×3 cluster: the centre has no healthy 4-neighbour on the first pass.
        for y in 2..5 {
            for x in 2..5 {
                f.data[y * w + x] = 0.0;
                d[y * w + x] = 1.0;
            }
        }
        fix_defects(&mut f, &Frame::new(w, h, d.clone()), 1, 1);
        assert_eq!(f.data[3 * w + 3], 0.0, "one pass leaves the centre");
        assert_eq!(f.data[2 * w + 2], 100.0);
        fix_defects(&mut f, &Frame::new(w, h, d), 1, 200);
        assert!(f.data.iter().all(|&v| v == 100.0));
    }

    #[test]
    fn calibration_files_round_trip() {
        let m = SensorModel { code: 0, name: "test", width: 4, height: 3, corner: 0 };
        let n = m.width * m.height;
        let mut gn = vec![0u8; CALIBRATION_HEADER];
        for k in 0..4u16 {
            for i in 0..n as u16 {
                gn.extend_from_slice(&(1000 * (k + 1) + i).to_le_bytes());
            }
        }
        let gains = parse_gain_file(&gn, m).unwrap();
        assert_eq!(gains.iter().map(|g| g.level).collect::<Vec<_>>(), [1000, 2000, 3000, 4000]);
        // A file of one flat field is header + w·h·2 bytes, below the w·h·4
        // cut for any real sensor (the test model above is too small for that).
        let big = SensorModel { width: 30, height: 20, ..m };
        let one = vec![0u8; CALIBRATION_HEADER + 600 * 2];
        assert_eq!(parse_gain_file(&one, big).unwrap().len(), 1);

        let mut dft = vec![0u8; CALIBRATION_HEADER + n];
        dft[CALIBRATION_HEADER + 5] = 1;
        let defects = parse_defect_file(&dft, m).unwrap();
        assert_eq!(defects.data.iter().filter(|v| **v != 0.0).count(), 1);
        assert!(parse_defect_file(&dft[..20], m).is_err());

        let dir = std::env::temp_dir().join(format!("lumodent-cal-{}", std::process::id()));
        let cal = dir.join("Correct").join("Default");
        std::fs::create_dir_all(&cal).unwrap();
        std::fs::write(cal.join("sensor.gn"), &gn).unwrap();
        std::fs::write(cal.join("sensor.dft"), &dft).unwrap();
        let loaded = load_calibration(&dir, m).unwrap();
        assert_eq!((loaded.gains.len(), loaded.defects.is_some()), (4, true));
        let raw: Vec<u8> = (0..n).flat_map(|_| 2000u16.to_le_bytes()).collect();
        let out = post_process(&parse_raw_frame(&raw, m).unwrap(), &loaded).unwrap();
        assert_eq!(out.data.len(), n);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn exposure_mask_skips_the_top_chamfers() {
        let mut data = vec![30000.0; 100 * 100];
        data[0] = 65000.0; // top-left corner, outside the counted area
        data[5050] = 65000.0;
        data[2020] = 0.0;
        let mask = exposure_mask(&Frame::new(100, 100, data), 10);
        assert!(!mask[0] && mask[5050] && mask[2020]);
        assert_eq!(mask.iter().filter(|m| **m).count(), 2);
        assert_eq!(to_image(&Frame::new(1, 1, vec![30000.0])).pixels[0], 117 * 0x010101);
    }
}
