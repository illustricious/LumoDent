//! Radiograph pixels and the viewer operations named in the reference executable
//! (RotateImage, FlipY, Invert, BrightnessAndContrast). The algorithms are
//! rewrite choices; the original pixel math has not been mapped yet.

/// Top-down pixels packed as 0x00RRGGBB, the layout GDI accepts for 32-bit DIBs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

pub(crate) const MAX_PIXELS: usize = 1 << 27;

/// Decodes DICOM and BMP with the built-in readers and PNG or JPEG with the image crate.
pub fn decode(data: &[u8]) -> Result<Image, String> {
    if crate::dicom::is_dicom(data) {
        return crate::dicom::parse(data).map(|d| d.image);
    }
    if data.starts_with(b"BM") {
        return decode_bmp(data);
    }
    // PNG and JPEG go through the pure-Rust image crate on every platform.
    let decoded = ::image::load_from_memory(data).map_err(|e| format!("Cannot decode image: {e}"))?;
    let (width, height) = (decoded.width() as usize, decoded.height() as usize);
    if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS) {
        return Err("Image is empty or too large".into());
    }
    let pixels = decoded
        .to_rgb8()
        .pixels()
        .map(|p| u32::from_be_bytes([0, p[0], p[1], p[2]]))
        .collect();
    Ok(Image { width, height, pixels })
}

pub fn decode_bmp(data: &[u8]) -> Result<Image, String> {
    let bad = || "Unsupported or damaged BMP file".to_string();
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(bad);
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(bad)
    };
    if data.get(..2) != Some(b"BM") {
        return Err("Not a BMP file".into());
    }
    let offset = u32_at(10)? as usize;
    let header = u32_at(14)? as usize;
    if header < 40 {
        return Err(bad());
    }
    let width = u32_at(18)? as i32;
    let height = u32_at(22)? as i32;
    let bpp = u16_at(28)? as usize;
    let compression = u32_at(30)?;
    if width <= 0 || height == 0 || height == i32::MIN {
        return Err(bad());
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    if w.checked_mul(h).map_or(true, |n| n > MAX_PIXELS) {
        return Err("BMP image is too large".into());
    }

    // BI_BITFIELDS masks follow a 40-byte header or live inside a larger one.
    let masks = match (bpp, compression) {
        (16 | 32, 3) => Some([u32_at(54)?, u32_at(58)?, u32_at(62)?]),
        (16, 0) => Some([0x7c00, 0x03e0, 0x001f]),
        (32, 0) => Some([0x00ff_0000, 0x0000_ff00, 0x0000_00ff]),
        (1 | 4 | 8 | 24, 0) => None,
        _ => return Err(bad()),
    };
    let palette: Vec<u32> = if bpp <= 8 {
        let used = match u32_at(46)? as usize {
            0 => 1 << bpp,
            n => n.min(1 << bpp),
        };
        let start = 14 + header;
        (0..used)
            .map(|i| u32_at(start + i * 4).map(|c| c & 0x00ff_ffff))
            .collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };

    let stride = (w * bpp).div_ceil(32) * 4;
    let end = stride.checked_mul(h).and_then(|n| n.checked_add(offset)).ok_or_else(bad)?;
    if end > data.len() {
        return Err(bad());
    }
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let source = if height < 0 { y } else { h - 1 - y };
        let row = &data[offset + source * stride..][..stride];
        for x in 0..w {
            pixels.push(match bpp {
                1 | 4 | 8 => {
                    let bit = x * bpp;
                    let index = (row[bit / 8] >> (8 - bpp - bit % 8)) & ((1u16 << bpp) - 1) as u8;
                    *palette.get(index as usize).ok_or_else(bad)?
                }
                24 => u32::from_le_bytes([row[x * 3], row[x * 3 + 1], row[x * 3 + 2], 0]),
                16 => unpack(u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]) as u32, masks.unwrap()),
                _ => unpack(
                    u32::from_le_bytes(row[x * 4..x * 4 + 4].try_into().unwrap()),
                    masks.unwrap(),
                ),
            });
        }
    }
    Ok(Image { width: w, height: h, pixels })
}

fn unpack(value: u32, masks: [u32; 3]) -> u32 {
    masks.iter().fold(0, |rgb, &mask| {
        let channel = if mask == 0 {
            0
        } else {
            let max = (1u64 << mask.count_ones()) - 1;
            let raw = ((value & mask) >> mask.trailing_zeros()) as u64;
            ((raw * 255 + max / 2) / max) as u32
        };
        rgb << 8 | channel
    })
}

impl Image {
    pub fn rotate_clockwise(&mut self) {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                out[x * h + (h - 1 - y)] = self.pixels[y * w + x];
            }
        }
        self.pixels = out;
        (self.width, self.height) = (h, w);
    }

    pub fn rotate_counter_clockwise(&mut self) {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                out[(w - 1 - x) * h + y] = self.pixels[y * w + x];
            }
        }
        self.pixels = out;
        (self.width, self.height) = (h, w);
    }

    /// Writes the image as a PNG file.
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        let rgb: Vec<u8> = self.pixels.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
        ::image::save_buffer(path, &rgb, self.width as u32, self.height as u32, ::image::ExtendedColorType::Rgb8)
            .map_err(|e| format!("Fail to save the image: {e}"))
    }

    pub fn flip_horizontal(&mut self) {
        for row in self.pixels.chunks_mut(self.width) {
            row.reverse();
        }
    }

    pub fn flip_vertical(&mut self) {
        let w = self.width;
        for y in 0..self.height / 2 {
            let (top, bottom) = self.pixels.split_at_mut((self.height - 1 - y) * w);
            top[y * w..(y + 1) * w].swap_with_slice(&mut bottom[..w]);
        }
    }

    pub fn invert(&mut self) {
        for p in &mut self.pixels {
            *p ^= 0x00ff_ffff;
        }
    }

    /// Applies a per-channel tone curve built by [`levels`].
    pub fn adjusted(&self, curve: &[u8; 256]) -> Vec<u32> {
        self.pixels
            .iter()
            .map(|&p| {
                let c = |shift: u32| (curve[(p >> shift & 0xff) as usize] as u32) << shift;
                c(16) | c(8) | c(0)
            })
            .collect()
    }
}

/// Brightness and contrast range from -100 to 100. Contrast scales around mid-grey,
/// from flat at -100 to four times the slope at 100.
pub fn levels(brightness: i32, contrast: i32) -> [u8; 256] {
    let (b, c) = (brightness.clamp(-100, 100) as f32, contrast.clamp(-100, 100) as f32);
    let slope = if c >= 0.0 { 1.0 + c * 0.03 } else { 1.0 + c / 100.0 };
    std::array::from_fn(|v| ((v as f32 - 127.5) * slope + 127.5 + b * 2.55).round().clamp(0.0, 255.0) as u8)
}

/// Zoom and pan relative to fitting the whole image inside the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub zoom: f32,
    pub pan: (f32, f32),
}

impl Default for Viewport {
    fn default() -> Self {
        Self { zoom: 1.0, pan: (0.0, 0.0) }
    }
}

impl Viewport {
    pub const MIN_ZOOM: f32 = 0.1;
    pub const MAX_ZOOM: f32 = 32.0;

    /// Returns the image's top-left corner on the canvas and its scale.
    pub fn place(&self, image: (usize, usize), canvas: (i32, i32)) -> (f32, f32, f32) {
        let (iw, ih) = (image.0.max(1) as f32, image.1.max(1) as f32);
        let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
        let scale = (cw / iw).min(ch / ih) * self.zoom;
        ((cw - iw * scale) / 2.0 + self.pan.0, (ch - ih * scale) / 2.0 + self.pan.1, scale)
    }

    /// Zooms by `factor` while keeping the image point under `at` in place.
    pub fn zoom_at(&mut self, factor: f32, image: (usize, usize), canvas: (i32, i32), at: (f32, f32)) {
        let (x, y, scale) = self.place(image, canvas);
        let point = ((at.0 - x) / scale, (at.1 - y) / scale);
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let (cx, cy, new_scale) = self.place(image, canvas);
        self.pan.0 += at.0 - (cx + point.0 * new_scale);
        self.pan.1 += at.1 - (cy + point.1 * new_scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_are_inverse() {
        let start = Image { width: 3, height: 2, pixels: (0..6).collect() };
        let mut i = start.clone();
        i.rotate_clockwise();
        assert_eq!((i.width, i.height, i.pixels[0]), (2, 3, 3));
        i.rotate_counter_clockwise();
        assert_eq!(i.pixels, start.pixels);
    }

    fn bmp(bpp: u16, width: i32, height: i32, palette: &[u32], rows: &[&[u8]]) -> Vec<u8> {
        let offset = 54 + palette.len() * 4;
        let mut d = b"BM".to_vec();
        let size = offset + rows.iter().map(|r| r.len()).sum::<usize>();
        for v in [size as u32, 0, offset as u32, 40, width as u32, height as u32] {
            d.extend(v.to_le_bytes());
        }
        d.extend(1u16.to_le_bytes());
        d.extend(bpp.to_le_bytes());
        for v in [0u32, 0, 0, 0, palette.len() as u32, 0] {
            d.extend(v.to_le_bytes());
        }
        for c in palette {
            d.extend(c.to_le_bytes());
        }
        for r in rows {
            d.extend(*r);
        }
        d
    }

    #[test]
    fn decodes_bottom_up_24_bit_rows_with_padding() {
        // Bottom row first: blue, green; then top row: red, white. Each row pads 6 to 8 bytes.
        let data = bmp(24, 2, 2, &[], &[&[255, 0, 0, 0, 255, 0, 0, 0], &[0, 0, 255, 255, 255, 255, 0, 0]]);
        let image = decode_bmp(&data).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.pixels, [0xff0000, 0xffffff, 0x0000ff, 0x00ff00]);
    }

    #[test]
    fn decodes_top_down_palettes_and_rejects_damage() {
        let grey = [0x000000, 0x808080, 0xffffff];
        let data = bmp(8, 3, -1, &grey, &[&[2, 1, 0, 0]]);
        assert_eq!(decode_bmp(&data).unwrap().pixels, [0xffffff, 0x808080, 0]);
        let mono = bmp(1, 3, 1, &[0, 0xffffff], &[&[0b1010_0000, 0, 0, 0]]);
        assert_eq!(decode_bmp(&mono).unwrap().pixels, [0xffffff, 0, 0xffffff]);
        let bad_index = bmp(8, 1, 1, &grey, &[&[7, 0, 0, 0]]);
        assert!(decode_bmp(&bad_index).is_err());
        assert!(decode_bmp(&data[..data.len() - 1]).is_err());
        assert!(decode_bmp(b"PNG").is_err());
        let rgb16 = bmp(16, 1, 1, &[], &[&[0x1f, 0x7c, 0, 0]]);
        assert_eq!(decode_bmp(&rgb16).unwrap().pixels, [0xff00ff]);
    }

    #[test]
    fn rotations_flips_and_inversion() {
        // 3 x 2:  1 2 3 / 4 5 6
        let original = Image { width: 3, height: 2, pixels: vec![1, 2, 3, 4, 5, 6] };
        let mut image = original.clone();
        image.rotate_clockwise();
        assert_eq!((image.width, image.height, image.pixels.clone()), (2, 3, vec![4, 1, 5, 2, 6, 3]));
        for _ in 0..3 {
            image.rotate_clockwise();
        }
        assert_eq!(image, original);
        image.flip_horizontal();
        assert_eq!(image.pixels, [3, 2, 1, 6, 5, 4]);
        image.flip_vertical();
        assert_eq!(image.pixels, [6, 5, 4, 3, 2, 1]);
        image.invert();
        assert_eq!(image.pixels[0], 0xffffff ^ 6);
    }

    #[test]
    fn tone_curve_endpoints() {
        let neutral = levels(0, 0);
        assert!(neutral.iter().enumerate().all(|(i, &v)| v as usize == i));
        assert_eq!(levels(100, 0)[0], 255);
        assert_eq!(levels(-100, 0)[255], 0);
        let flat = levels(0, -100);
        assert!(flat.iter().all(|&v| v == 128));
        let steep = levels(0, 100);
        assert_eq!((steep[64], steep[192]), (0, 255));
        let image = Image { width: 1, height: 1, pixels: vec![0x102030] };
        assert_eq!(image.adjusted(&levels(100, 0)), [0xffffff]);
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let mut view = Viewport::default();
        // 200 x 100 image in a 400 x 400 canvas fits at scale 2, centred vertically.
        assert_eq!(view.place((200, 100), (400, 400)), (0.0, 100.0, 2.0));
        let before = view.place((200, 100), (400, 400));
        let at = (100.0, 150.0);
        let point = ((at.0 - before.0) / before.2, (at.1 - before.1) / before.2);
        view.zoom_at(2.0, (200, 100), (400, 400), at);
        let (x, y, s) = view.place((200, 100), (400, 400));
        assert_eq!(s, 4.0);
        assert!((x + point.0 * s - at.0).abs() < 1e-3 && (y + point.1 * s - at.1).abs() < 1e-3);
        view.zoom_at(1e9, (200, 100), (400, 400), at);
        assert_eq!(view.zoom, Viewport::MAX_ZOOM);
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn decodes_png_exactly() {
        let image = decode(include_bytes!("../tests/fixtures/rgb-3x2.png")).unwrap();
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(image.pixels, [0xff0000, 0x00ff00, 0x0000ff, 0xffffff, 0x000000, 0x808080]);
    }

    #[test]
    fn decodes_jpeg_within_compression_error() {
        let image = decode(include_bytes!("../tests/fixtures/halves-32x16.jpg")).unwrap();
        assert_eq!((image.width, image.height), (32, 16));
        let grey = |p: u32| (p & 0xff) as i32;
        for y in 0..16 {
            assert!(grey(image.pixels[y * 32 + 2]) < 24);
            assert!(grey(image.pixels[y * 32 + 29]) > 231);
        }
    }

    #[test]
    fn rejects_unknown_bytes() {
        assert!(decode(b"DICM not an image").is_err());
        assert!(decode(&include_bytes!("../tests/fixtures/rgb-3x2.png")[..40]).is_err());
    }
}
