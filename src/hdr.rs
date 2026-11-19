//! Image correction for the NanoPix-E 1.3 sensor, the job of the reference
//! HdrImageCorrect.dll (`Sensor_CorrectImage`) and the ShadoCam routines it
//! calls in Scilib21.dll. Frames are width × height 16-bit counts, row-major;
//! calibration files are big-endian on disk. Detector I/O (sensor_io.dll) is
//! not part of this module.

use std::path::{Path, PathBuf};

/// The two read-out channels of an HDR frame differ in gain by this factor
/// (a literal in HdrImageCorrect 0x10002000).
pub const HDR_RATIO: f64 = 9.19;

// ---- calibration files ----------------------------------------------------

/// `<dir>\<serial>_<name>`, the reference's `%s\%s_Offset Image` pattern.
pub fn calibration_path(dir: &Path, serial: &str, name: &str) -> PathBuf {
    dir.join(format!("{serial}_{name}"))
}

/// A raw calibration image: width × height big-endian 16-bit values. The
/// reference ignores a file of the wrong size (it skips that correction).
pub fn parse_image_file(bytes: &[u8], width: usize, height: usize) -> Option<Vec<u16>> {
    (bytes.len() == width * height * 2).then(|| bytes.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect())
}

// ---- ScOffsetCorrection / ScGainCorrection (Scilib21) ------------------------

/// ScOffsetCorrection (0x100024d0): pixel − offset, 16-bit wrapping.
pub fn offset_correction(image: &mut [u16], offset: &[u16]) {
    for (v, o) in image.iter_mut().zip(offset) {
        *v = v.wrapping_sub(*o);
    }
}

/// ScGainCorrection (0x10002510) with mean 0: the mean of the gain image
/// (values read as signed 16-bit) is computed, then each pixel becomes
/// pixel × mean / gain, truncated toward zero; a zero gain gives 0.
pub fn gain_correction(image: &mut [u16], gain: &[u16]) {
    let n = gain.len().min(image.len());
    if n == 0 {
        return;
    }
    let mean = gain[..n].iter().map(|&g| g as i16 as f64).sum::<f64>() / n as f64;
    for (v, &g) in image.iter_mut().zip(gain) {
        *v = if g == 0 { 0 } else { ((*v as i16 as f64) * mean / (g as i16 as f64)) as i64 as u16 };
    }
}

// ---- ShadoCam pixel map (ScReadPixMap / ScCalcPixMapMask) ----------------------

const PIXMAP_HEADER: &[u8] = b"ShadoCam Pixel Map File";
const PIXMAP_BEGIN: &[u8] = b"$begin";

/// Neighbour bits of the defect mask: which of the 8 neighbours are defective.
pub mod bits {
    pub const UP_LEFT: u8 = 0x01;
    pub const LEFT: u8 = 0x02;
    pub const DOWN_LEFT: u8 = 0x04;
    pub const DOWN: u8 = 0x08;
    pub const DOWN_RIGHT: u8 = 0x10;
    pub const RIGHT: u8 = 0x20;
    pub const UP_RIGHT: u8 = 0x40;
    pub const UP: u8 = 0x80;
    pub const LEFT_COLUMN: u8 = 0x07;
    pub const RIGHT_COLUMN: u8 = 0x70;
    pub const TOP_ROW: u8 = 0xc1;
    pub const BOTTOM_ROW: u8 = 0x1c;
}

/// One pixel-map entry, 1-based coordinates as in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defect {
    /// A column `x`, rows `first..=last` (0, 0 = the whole column).
    Column { x: u16, first: u16, last: u16, plain: bool, mask: u8 },
    Pixel { x: u16, y: u16, mask: u8 },
    /// A row `y`, columns `first..=last` (0, 0 = the whole row).
    Row { y: u16, first: u16, last: u16, plain: bool, mask: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixMapError {
    /// The file is empty or larger than 0x19000 bytes, or the header is wrong (code 7).
    Unreadable,
    /// An entry with a zero coordinate (code 8); the other entries are kept.
    Syntax,
}

/// strtoul(…, 10): optional whitespace and sign, digits; returns the value
/// and the end position (the start when there are no digits).
fn strtoul(s: &[u8], mut i: usize) -> (u32, usize) {
    let start = i;
    while i < s.len() && (s[i] == b' ' || (9..=13).contains(&s[i])) {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    let digits = i;
    let mut v: u32 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((s[i] - b'0') as u32);
        i += 1;
    }
    if i == digits {
        return (0, start);
    }
    (if neg { v.wrapping_neg() } else { v }, i)
}

/// ScReadPixMap (0x100036a0). The text starts with "ShadoCam Pixel Map File",
/// entries follow "$begin". Each `C`, `P` or `R` starts an entry; any other
/// character is skipped. Numbers are read the way the reference does: after a
/// non-zero value one separator is skipped, after a zero none.
pub fn parse_pixel_map(text: &[u8]) -> Result<(Vec<Defect>, bool), PixMapError> {
    if text.is_empty() || text.len() > 0x19000 || !text.starts_with(PIXMAP_HEADER) {
        return Err(PixMapError::Unreadable);
    }
    let begin = text.windows(PIXMAP_BEGIN.len()).position(|w| w == PIXMAP_BEGIN).ok_or(PixMapError::Unreadable)?;
    let mut i = begin + PIXMAP_BEGIN.len();
    let mut out = Vec::new();
    let mut syntax = false;
    // Reads a value and moves past it (plus one separator) only when it is non-zero.
    let next = |i: &mut usize| -> u16 {
        let (v, end) = strtoul(text, *i);
        if v as u16 != 0 {
            *i = end + 1;
        }
        v as u16
    };
    while i < text.len() && out.len() < 10000 {
        let c = text[i];
        i += 1;
        match c {
            b'C' => {
                let x = next(&mut i);
                if x == 0 {
                    syntax = true;
                    continue;
                }
                let (mut first, mut last) = (next(&mut i), next(&mut i));
                if last <= first {
                    (first, last) = (0, 0);
                }
                let plain = strtoul(text, i).0 as u8 != 0;
                out.push(Defect::Column { x, first, last, plain, mask: 0 });
            }
            b'P' => {
                let x = next(&mut i);
                if x == 0 {
                    syntax = true;
                    continue;
                }
                let y = next(&mut i);
                if y == 0 {
                    syntax = true;
                    continue;
                }
                out.push(Defect::Pixel { x, y, mask: 0 });
            }
            b'R' => {
                let y = next(&mut i);
                if y == 0 {
                    syntax = true;
                    continue;
                }
                let (mut first, mut last) = (next(&mut i), next(&mut i));
                if last <= first {
                    (first, last) = (0, 0);
                }
                let plain = strtoul(text, i).0 as u8 != 0;
                out.push(Defect::Row { y, first, last, plain, mask: 0 });
            }
            _ => {}
        }
    }
    calc_masks(&mut out);
    Ok((out, syntax))
}

/// ScCalcPixMapMask (0x10002c30): marks which neighbours of each entry are
/// themselves defective. As in the reference, a pixel only looks at entries
/// listed after it.
pub fn calc_masks(map: &mut [Defect]) {
    use bits::*;
    let covers = |first: u16, last: u16, v: u16| first == last || (first <= v && v <= last);
    let snapshot = map.to_vec();
    for (k, entry) in map.iter_mut().enumerate() {
        match entry {
            Defect::Column { x, mask, .. } => {
                let mut m = UP | DOWN;
                for e in &snapshot {
                    if let Defect::Column { x: ox, .. } = e {
                        if *ox as u32 == *x as u32 - 1 { m |= LEFT_COLUMN; }
                        if *ox as u32 == *x as u32 + 1 { m |= RIGHT_COLUMN; }
                    }
                }
                *mask = m;
            }
            Defect::Row { y, mask, .. } => {
                let mut m = LEFT | RIGHT;
                for e in &snapshot {
                    if let Defect::Row { y: oy, .. } = e {
                        if *oy as u32 == *y as u32 - 1 { m |= TOP_ROW; }
                        if *oy as u32 == *y as u32 + 1 { m |= BOTTOM_ROW; }
                    }
                }
                *mask = m;
            }
            Defect::Pixel { x, y, mask } => {
                let (x, y) = (*x as u32, *y as u32);
                let mut m = 0u8;
                for e in &snapshot[k..] {
                    match *e {
                        Defect::Column { x: ox, first, last, .. } => {
                            if ox as u32 == x - 1 && covers(first, last, y as u16) { m |= LEFT_COLUMN; }
                            if ox as u32 == x + 1 && covers(first, last, y as u16) { m |= RIGHT_COLUMN; }
                        }
                        Defect::Row { y: oy, first, last, .. } => {
                            if oy as u32 == y - 1 && covers(first, last, x as u16) { m |= TOP_ROW; }
                            if oy as u32 == y + 1 && covers(first, last, x as u16) { m |= BOTTOM_ROW; }
                        }
                        Defect::Pixel { x: ox, y: oy, .. } => {
                            let (ox, oy) = (ox as u32, oy as u32);
                            if ox == x - 1 && oy == y - 1 { m |= UP_LEFT; }
                            if ox == x - 1 && oy == y { m |= LEFT; }
                            if ox == x - 1 && oy == y + 1 { m |= DOWN_LEFT; }
                            if ox == x && oy == y + 1 { m |= DOWN; }
                            if ox == x + 1 && oy == y + 1 { m |= DOWN_RIGHT; }
                            if ox == x + 1 && oy == y { m |= RIGHT; }
                            if ox == x + 1 && oy == y - 1 { m |= UP_RIGHT; }
                            if ox == x && oy == y - 1 { m |= UP; }
                        }
                    }
                }
                *mask = m;
            }
        }
    }
}

// ---- ScFixPixel / ScPixelCorrection ------------------------------------------

struct Img<'a> {
    d: &'a mut [u16],
    w: i64,
    h: i64,
}

impl Img<'_> {
    /// The reference reads pixels as signed 16-bit.
    fn at(&self, x: i64, y: i64) -> i64 {
        self.d[(y * self.w + x) as usize] as i16 as i64
    }
}

/// ScFixPixel (0x10002620). `method`: 0 = mean of the healthy 8-neighbours,
/// 1 = horizontal, 2 = vertical, 3 = edge-directed across a column,
/// 4 = edge-directed across a row.
fn fix_pixel(img: &mut Img, x: i64, y: i64, method: u32, mask: u8) {
    use bits::*;
    if x < 0 || x >= img.w || y < 0 || y >= img.h || method > 4 {
        return;
    }
    let (mut sum, mut n): (i64, i64) = (0, 0);
    let mut add = |v: i64, weight: i64| {
        sum += v * weight;
        n += weight;
    };
    match method {
        1 => {
            if mask & LEFT == 0 { add(img.at(x - 1, y), 2) } else { add(img.at(x - 2, y), 1) }
            if mask & RIGHT == 0 { add(img.at(x + 1, y), 2) } else { add(img.at(x + 2, y), 1) }
        }
        2 => {
            if mask & UP == 0 { add(img.at(x, y - 1), 2) } else { add(img.at(x, y - 2), 1) }
            if mask & DOWN == 0 { add(img.at(x, y + 1), 2) } else { add(img.at(x, y + 2), 1) }
        }
        3 | 4 => {
            // a = how far the healthy side before the defect is, b after.
            let (a, b) = if method == 3 {
                (if mask & LEFT_COLUMN != 0 { 2 } else { 1 }, if mask & RIGHT_COLUMN != 0 { 2 } else { 1 })
            } else {
                (if mask & TOP_ROW != 0 { 2 } else { 1 }, if mask & BOTTOM_ROW != 0 { 2 } else { 1 })
            };
            // Three candidate directions; each compares three pixel pairs.
            let mut d = [0f64; 3];
            for (k, dk) in (-1i64..=1).zip(d.iter_mut()) {
                for t in -1..=1 {
                    let diff = if method == 3 {
                        img.at(x - a, y + k * a + t) - img.at(x + b, y - k * b + t)
                    } else {
                        img.at(x + k * a + t, y - a) - img.at(x - k * b + t, y + b)
                    } as f64;
                    *dk += diff * diff;
                }
            }
            let min = d[1].min(d[2]).min(d[0]);
            let k = d.iter().position(|&v| v == min).unwrap_or(1) as i64 - 1;
            let v = if method == 3 {
                img.at(x - a, y + k * a) * (3 - a) + img.at(x + b, y - k * b) * (3 - b)
            } else {
                img.at(x - k * b, y + b) * (3 - b) + img.at(x + k * a, y - a) * (3 - a)
            };
            sum = v;
            n = 6 - a - b;
        }
        _ => {
            let neighbours = [
                (UP_LEFT, -1, -1), (LEFT, -1, 0), (DOWN_LEFT, -1, 1), (DOWN, 0, 1),
                (DOWN_RIGHT, 1, 1), (RIGHT, 1, 0), (UP_RIGHT, 1, -1), (UP, 0, -1),
            ];
            for (bit, dx, dy) in neighbours {
                if mask & bit == 0 {
                    add(img.at(x + dx, y + dy), 1);
                }
            }
        }
    }
    let i = (y * img.w + x) as usize;
    img.d[i] = if n == 0 { 0 } else { (sum / n) as u16 };
}

/// ScPixelCorrection (0x10003220) with the given method (0 = neighbour mean
/// everywhere, 1 = directional along lines, 2 = edge-directed along lines;
/// the reference uses 2).
pub fn pixel_correction(image: &mut [u16], width: usize, height: usize, map: &[Defect], method: u16) {
    use bits::*;
    if width * height == 0 || image.len() < width * height || method > 2 {
        return;
    }
    let (w, h) = (width as i64, height as i64);
    let mut img = Img { d: image, w, h };
    for entry in map {
        match *entry {
            Defect::Column { x, first, last, plain, mask } => {
                let x = x as i64 - 1;
                let (mut y0, mut y1) = (first as i64 - 1, last as i64 - 1);
                if y0 == y1 {
                    (y0, y1) = (0, h - 1);
                }
                y1 = y1.min(h - 1);
                if y1 < y0 {
                    y0 = 0;
                }
                let mut dir = if method == 0 || plain { 0 } else { 1 };
                let mut m = mask;
                if x >= w {
                    continue;
                }
                if x == 0 {
                    dir = 0;
                    m |= LEFT_COLUMN;
                } else if x == 1 && m & LEFT_COLUMN == LEFT_COLUMN {
                    dir = 0;
                }
                if x == w - 1 {
                    dir = 0;
                    m |= RIGHT_COLUMN;
                }
                if x == w - 2 && m & RIGHT_COLUMN == RIGHT_COLUMN {
                    dir = 0;
                }
                // The first and last three rows use the simpler direction.
                if y0 <= 2 {
                    for y in y0..3 {
                        fix_pixel(&mut img, x, y, dir, if y == 0 { m | TOP_ROW } else { m });
                    }
                    y0 = 3;
                }
                for k in 0..3 {
                    if y1 == h - 1 - k {
                        fix_pixel(&mut img, x, y1, dir, if k == 0 { m | BOTTOM_ROW } else { m });
                        y1 -= 1;
                    }
                }
                if method == 2 && dir != 0 {
                    dir = 3;
                }
                for y in y0..=y1 {
                    fix_pixel(&mut img, x, y, dir, m);
                }
            }
            Defect::Pixel { x, y, mask } => {
                let (x, y) = (x as i64 - 1, y as i64 - 1);
                if x >= w || y >= h {
                    continue;
                }
                let mut m = mask;
                if x == 0 { m |= LEFT_COLUMN; }
                if x == w - 1 { m |= RIGHT_COLUMN; }
                if y == 0 { m |= TOP_ROW; }
                if y == h - 1 { m |= BOTTOM_ROW; }
                fix_pixel(&mut img, x, y, 0, m);
            }
            Defect::Row { y, first, last, plain, mask } => {
                let y = y as i64 - 1;
                let (mut x0, mut x1) = (first as i64 - 1, last as i64 - 1);
                if x0 == x1 {
                    (x0, x1) = (0, w - 1);
                }
                x1 = x1.min(w - 1);
                if x1 < x0 {
                    x0 = 0;
                }
                let mut dir = if method == 0 || plain { 0 } else { 2 };
                let mut m = mask;
                if y >= h {
                    continue;
                }
                if y == 0 {
                    dir = 0;
                    m |= TOP_ROW;
                } else if y == 1 && m & TOP_ROW == TOP_ROW {
                    dir = 0;
                }
                if y == h - 1 {
                    dir = 0;
                    m |= BOTTOM_ROW;
                }
                if y == h - 2 && m & BOTTOM_ROW == BOTTOM_ROW {
                    dir = 0;
                }
                if x0 <= 2 {
                    for x in x0..3 {
                        fix_pixel(&mut img, x, y, dir, if x == 0 { m | LEFT_COLUMN } else { m });
                    }
                    x0 = 3;
                }
                for k in 0..3 {
                    if x1 == w - 1 - k {
                        fix_pixel(&mut img, x1, y, dir, if k == 0 { m | RIGHT_COLUMN } else { m });
                        x1 -= 1;
                    }
                }
                if method == 2 && dir != 0 {
                    dir = 4;
                }
                for x in x0..=x1 {
                    fix_pixel(&mut img, x, y, dir, m);
                }
            }
        }
    }
}

// ---- HDR merge (HdrImageCorrect 0x10002000) -----------------------------------

/// Calibration for the HDR path: an offset and a gain image per channel.
pub struct HdrCalibration {
    pub offset0: Vec<u16>,
    pub offset1: Vec<u16>,
    pub gain0: Vec<u16>,
    pub gain1: Vec<u16>,
}

/// Bits per pixel the reference infers from the mean of the first ten rows of
/// the channel-0 offset: ≥ 1500 → 16, ≥ 400 → 14, else 12.
pub fn detect_bit_depth(offset0: &[u16], width: usize) -> u32 {
    let n = (10 * width).min(offset0.len()).max(1);
    let mean = offset0[..n].iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    if mean >= 1500.0 { 16 } else if mean >= 400.0 { 14 } else { 12 }
}

fn round_half_away(v: f64) -> f64 {
    if v >= 0.0 { (v + 0.5).floor() } else { (v - 0.5).ceil() }
}

/// Merges an HDR frame. The lowest bit of each raw pixel says which channel it
/// was read from (odd = channel 1, the low-gain one, 9.19× less sensitive).
///
/// 1. Isolated odd pixels (no odd neighbour in 3×3) pair their channel-1 level
///    with the mean channel-0 level of their 8 neighbours; the mean
///    difference, plus 200/50/12 for 16/14/12-bit, is the channel-1 offset.
/// 2. Each pixel is halved and offset-corrected: odd ones as
///    (v − offset1) × 9.19, even ones as v − offset0, clamped at 0.
/// 3. Flat-field gains per channel are normalised to their mean (values under
///    0.1 become 1) and divided out.
/// 4. Odd pixels get the step-1 offset; everything is halved again, rounded
///    half away from zero and limited to the bit depth's maximum.
pub fn hdr_merge(image: &mut [u16], width: usize, height: usize, cal: &HdrCalibration) -> bool {
    let n = width * height;
    if n == 0 || image.len() < n || [&cal.offset0, &cal.offset1, &cal.gain0, &cal.gain1].iter().any(|c| c.len() < n) {
        return false;
    }
    let (w, h) = (width, height);
    let idx = |x: usize, y: usize| y * w + x;
    let depth = detect_bit_depth(&cal.offset0, w);
    let odd: Vec<bool> = image[..n].iter().map(|&v| v & 1 == 1).collect();
    let raw: Vec<f64> = image[..n].iter().map(|&v| v as f64).collect();
    let (off0, off1) = (&cal.offset0, &cal.offset1);

    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for x in 1..w.saturating_sub(1) {
        for y in 1..h.saturating_sub(1) {
            if !odd[idx(x, y)] {
                continue;
            }
            let (mut odd_count, mut acc) = (0, 0.0);
            for xn in x - 1..=x + 1 {
                for yn in y - 1..=y + 1 {
                    odd_count += odd[idx(xn, yn)] as i32;
                    acc += raw[idx(xn, yn)] * 0.5 - off0[idx(xn, yn)] as f64;
                }
            }
            let centre = raw[idx(x, y)] * 0.5;
            let neighbours = (off0[idx(x, y)] as f64 + acc - centre) * 0.125;
            if odd_count == 1 {
                xs.push(centre - off1[idx(x, y)] as f64);
                ys.push(neighbours);
            }
        }
    }
    let base = match depth { 16 => 200.0, 14 => 50.0, _ => 12.0 };
    let shift = if xs.len() > 1 {
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        mean(&ys) - mean(&xs) * HDR_RATIO + base
    } else {
        2000.0
    };

    let mut a: Vec<f64> = (0..n)
        .map(|i| {
            let v = raw[i] * 0.5;
            let v = if odd[i] { (v - off1[i] as f64) * HDR_RATIO } else { v - off0[i] as f64 };
            v.max(0.0)
        })
        .collect();

    let g0: Vec<f64> = (0..n).map(|i| cal.gain0[i] as f64 - off0[i] as f64).collect();
    let g1: Vec<f64> = (0..n).map(|i| (cal.gain1[i] as f64 - off1[i] as f64) * HDR_RATIO).collect();
    let m0 = g0.iter().sum::<f64>() / w as f64 / h as f64;
    let m1 = g1.iter().sum::<f64>() / w as f64 / h as f64;
    let norm = |g: f64, m: f64| {
        let v = g / m;
        if v < 0.1 { 1.0 } else { v }
    };
    let max = match depth { 16 => 0xffff, 14 => 0x3fff, _ => 0xfff } as f64;
    for i in 0..n {
        a[i] /= if odd[i] { norm(g1[i], m1) } else { norm(g0[i], m0) };
        if odd[i] {
            a[i] += shift;
        }
        let v = round_half_away(a[i] * 0.5);
        image[i] = if v > max { max as u16 } else { v as i64 as u16 };
    }
    true
}

// ---- Sensor_CorrectImage -------------------------------------------------------

/// Sensor_CorrectImage (0x10001060) error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectError {
    NoOffset = -1,
    NoGain = -2,
    BadPixelMap = -3,
}

/// The whole correction for a frame (native byte order) of a sensor whose
/// files are `<dir>\<serial>_Offset Image`, `_Gain Image` and `_Pixel Map`
/// (HDR: `_Offset Image0/1`, `_Gain Image0/1`).
///
/// One difference from the reference: it passes width × height (masked to
/// 15 bits) as the pixel-map entry count, so stale buffer contents can be read
/// as extra entries; here only the entries in the file are used.
pub fn correct_image(image: &mut [u16], width: usize, height: usize, dir: &Path, serial: &str, hdr: bool) -> Result<(), CorrectError> {
    let read = |name: &str| std::fs::read(calibration_path(dir, serial, name)).ok();
    if hdr {
        let load = |name: &str| read(name).and_then(|b| parse_image_file(&b, width, height));
        // The reference ignores the HDR merge's result and goes on to the pixel map.
        if let (Some(offset0), Some(offset1), Some(gain0), Some(gain1)) =
            (load("Offset Image0"), load("Offset Image1"), load("Gain Image0"), load("Gain Image1"))
        {
            hdr_merge(image, width, height, &HdrCalibration { offset0, offset1, gain0, gain1 });
        }
    } else {
        let offset = read("Offset Image").ok_or(CorrectError::NoOffset)?;
        if let Some(o) = parse_image_file(&offset, width, height) {
            offset_correction(image, &o);
        }
        let gain = read("Gain Image").ok_or(CorrectError::NoGain)?;
        if let Some(g) = parse_image_file(&gain, width, height) {
            gain_correction(image, &g);
        }
    }
    let text = read("Pixel Map").ok_or(CorrectError::BadPixelMap)?;
    let (map, syntax) = parse_pixel_map(&text).map_err(|_| CorrectError::BadPixelMap)?;
    if syntax {
        return Err(CorrectError::BadPixelMap);
    }
    pixel_correction(image, width, height, &map, 2);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_and_gain_follow_scilib() {
        let mut img = vec![1000u16, 2000, 3000, 4000];
        offset_correction(&mut img, &[100, 100, 100, 100]);
        assert_eq!(img, [900, 1900, 2900, 3900]);
        // Mean gain 300: pixel × 300 / gain, truncated; a zero gain zeroes the pixel.
        gain_correction(&mut img, &[200, 400, 600, 0]);
        assert_eq!(img, [1350, 1425, 1450, 0]);
    }

    #[test]
    fn pixel_map_parses_like_strtoul() {
        let text = b"ShadoCam Pixel Map File\r\n$begin\r\nC 5,10,20,0\r\nP 3,4\r\nR 7\r\nC 0\r\nP 6,7\r\n";
        let (map, syntax) = parse_pixel_map(text).unwrap();
        assert!(syntax, "C 0 is a syntax error");
        assert_eq!(map.len(), 4);
        assert!(matches!(map[0], Defect::Column { x: 5, first: 10, last: 20, plain: false, .. }));
        assert!(matches!(map[2], Defect::Row { y: 7, first: 0, last: 0, .. }));
        assert_eq!(parse_pixel_map(b"Not a pixel map"), Err(PixMapError::Unreadable));
    }

    #[test]
    fn masks_mark_defective_neighbours() {
        let mut map = vec![
            Defect::Column { x: 5, first: 0, last: 0, plain: false, mask: 0 },
            Defect::Column { x: 6, first: 0, last: 0, plain: false, mask: 0 },
            Defect::Pixel { x: 10, y: 10, mask: 0 },
            Defect::Pixel { x: 11, y: 10, mask: 0 },
        ];
        calc_masks(&mut map);
        let mask = |d: &Defect| match *d {
            Defect::Column { mask, .. } | Defect::Pixel { mask, .. } | Defect::Row { mask, .. } => mask,
        };
        assert_eq!(mask(&map[0]), bits::UP | bits::DOWN | bits::RIGHT_COLUMN);
        assert_eq!(mask(&map[1]), bits::UP | bits::DOWN | bits::LEFT_COLUMN);
        // Only later entries count for a pixel.
        assert_eq!(mask(&map[2]), bits::RIGHT);
        assert_eq!(mask(&map[3]), 0);
    }

    #[test]
    fn a_dead_column_is_filled_from_both_sides() {
        let (w, h) = (10, 10);
        // A horizontal ramp with column 5 (x = 4) dead.
        let mut img: Vec<u16> = (0..w * h).map(|i| (100 * (i % w)) as u16).collect();
        let want = img.clone();
        for y in 0..h {
            img[y * w + 4] = 0;
        }
        let mut map = vec![Defect::Column { x: 5, first: 0, last: 0, plain: false, mask: 0 }];
        calc_masks(&mut map);
        pixel_correction(&mut img, w, h, &map, 2);
        assert_eq!(img, want);
    }

    #[test]
    fn single_pixels_take_the_neighbour_mean() {
        let (w, h) = (5, 5);
        let mut img = vec![500u16; w * h];
        img[12] = 9000;
        let mut map = vec![Defect::Pixel { x: 3, y: 3, mask: 0 }];
        calc_masks(&mut map);
        pixel_correction(&mut img, w, h, &map, 2);
        assert_eq!(img[12], 500);
    }

    #[test]
    fn hdr_merge_puts_both_channels_on_one_scale() {
        let (w, h) = (8, 8);
        let n = w * h;
        // 16-bit sensor (offset ≥ 1500). Even pixels read 2·(1600 + s),
        // odd ones 2·(1500 + s / 9.19) with the lowest bit set.
        let s = 4000.0;
        let even = (2.0 * (1600.0 + s)) as u16 & !1;
        let odd = (2.0 * (1500.0 + s / HDR_RATIO)) as u16 | 1;
        let mut img = vec![even; n];
        for &i in &[18usize, 21, 42, 45] {
            img[i] = odd;
        }
        let cal = HdrCalibration { offset0: vec![1600; n], offset1: vec![1500; n], gain0: vec![3000; n], gain1: vec![3000; n] };
        assert_eq!(detect_bit_depth(&cal.offset0, w), 16);
        assert!(hdr_merge(&mut img, w, h, &cal));
        // Channel 0 comes out as s / 2.
        assert_eq!(img[0], (s / 2.0) as u16);
        // Channel 1 lands within the 200-count margin the reference adds.
        let diff = img[18] as f64 - img[0] as f64;
        assert!(diff.abs() <= 110.0, "odd {} vs even {}", img[18], img[0]);
    }

    #[test]
    fn correct_image_reads_the_three_files() {
        let (w, h) = (6, 4);
        let dir = std::env::temp_dir().join(format!("lumodent-hdr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let be = |v: u16| -> Vec<u8> { (0..w * h).flat_map(|_| v.to_be_bytes()).collect() };
        std::fs::write(calibration_path(&dir, "SN1", "Offset Image"), be(100)).unwrap();
        std::fs::write(calibration_path(&dir, "SN1", "Gain Image"), be(1000)).unwrap();
        std::fs::write(calibration_path(&dir, "SN1", "Pixel Map"), b"ShadoCam Pixel Map File\n$begin\nP 3,2\n").unwrap();
        let mut img = vec![1100u16; w * h];
        img[w + 2] = 60000;
        assert_eq!(correct_image(&mut img, w, h, &dir, "SN1", false), Ok(()));
        assert!(img.iter().all(|&v| v == 1000));
        assert_eq!(correct_image(&mut img, w, h, &dir, "SN2", false), Err(CorrectError::NoOffset));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
