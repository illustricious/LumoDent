//! The rest of the reference ImageProcessing library, written from scratch in
//! Rust: Set, MultiplyConstant, GetAverageValue, GetMinMax*, ImageNormalizeEx,
//! AutoBrightnessAndContrast, MedianSmooth, Smoothness, Sharpness, MultScaleWLN
//! and MatTransfor. Only the names come from the reference; the pixel math is
//! the rewrite's own. Every filter works on the three 8-bit channels separately,
//! so grey radiographs and colour photos both work.

use crate::image::Image;

type Plane = Vec<f32>;

fn split(image: &Image) -> [Plane; 3] {
    std::array::from_fn(|c| {
        let shift = 16 - 8 * c as u32;
        image.pixels.iter().map(|p| (p >> shift & 0xff) as f32).collect()
    })
}

fn join(image: &mut Image, planes: &[Plane; 3]) {
    let q = |v: f32| v.round().clamp(0.0, 255.0) as u32;
    for (i, p) in image.pixels.iter_mut().enumerate() {
        *p = q(planes[0][i]) << 16 | q(planes[1][i]) << 8 | q(planes[2][i]);
    }
}

/// Runs `f` on each channel plane; a grey image (all channels equal) is
/// processed once and copied, which makes filters three times faster on X-rays.
fn per_plane(image: &mut Image, f: impl Fn(Plane) -> Plane) {
    let grey = image.pixels.iter().all(|&p| p >> 16 & 0xff == p & 0xff && p >> 8 & 0xff == p & 0xff);
    if grey {
        let out = f(image.pixels.iter().map(|p| (p & 0xff) as f32).collect());
        for (p, v) in image.pixels.iter_mut().zip(out) {
            *p = v.round().clamp(0.0, 255.0) as u32 * 0x010101;
        }
    } else {
        let planes = split(image).map(f);
        join(image, &planes);
    }
}

fn map(image: &mut Image, f: impl Fn(f32) -> f32) {
    let curve: [u32; 256] = std::array::from_fn(|v| f(v as f32).round().clamp(0.0, 255.0) as u32);
    for p in &mut image.pixels {
        *p = curve[(*p >> 16 & 0xff) as usize] << 16 | curve[(*p >> 8 & 0xff) as usize] << 8 | curve[(*p & 0xff) as usize];
    }
}

fn luma(p: u32) -> u32 {
    ((p >> 16 & 0xff) * 77 + (p >> 8 & 0xff) * 150 + (p & 0xff) * 29) >> 8
}

/// Set: fills the image with one grey level.
pub fn set(image: &mut Image, value: u8) {
    let v = value as u32;
    image.pixels.fill(v << 16 | v << 8 | v);
}

/// MultiplyConstant: scales every channel, clamped to 0..255.
pub fn multiply_constant(image: &mut Image, factor: f32) {
    map(image, |v| v * factor);
}

/// GetAverageValue: mean brightness (luma).
pub fn average_value(image: &Image) -> f64 {
    if image.pixels.is_empty() {
        return 0.0;
    }
    image.pixels.iter().map(|&p| luma(p) as u64).sum::<u64>() as f64 / image.pixels.len() as f64
}

/// GetMinMaxIntegerValue: darkest and brightest luma.
pub fn min_max(image: &Image) -> (u8, u8) {
    image.pixels.iter().fold((255, 0), |(lo, hi), &p| {
        let l = luma(p) as u8;
        (lo.min(l), hi.max(l))
    })
}

/// ImageNormalizeEx: stretches the used range to the full 0..255.
pub fn normalize(image: &mut Image) {
    let (lo, hi) = min_max(image);
    stretch(image, lo as f32, hi as f32);
}

fn stretch(image: &mut Image, lo: f32, hi: f32) {
    if hi > lo {
        map(image, |v| (v - lo) * 255.0 / (hi - lo));
    }
}

/// AutoBrightnessAndContrast: like normalize, but ignores the darkest and
/// brightest `clip` percent of pixels so a few outliers don't block the stretch.
/// Returns the low and high levels it mapped to black and white.
pub fn auto_levels(image: &mut Image, clip: f32) -> (u8, u8) {
    let mut hist = [0usize; 256];
    for &p in &image.pixels {
        hist[luma(p) as usize] += 1;
    }
    let skip = (image.pixels.len() as f32 * clip.clamp(0.0, 49.0) / 100.0) as usize;
    let level = |order: &mut dyn Iterator<Item = usize>| {
        let mut seen = 0;
        for v in order {
            seen += hist[v];
            if seen > skip {
                return v;
            }
        }
        0
    };
    let lo = level(&mut (0..256));
    let hi = level(&mut (0..256).rev());
    stretch(image, lo as f32, hi as f32);
    (lo as u8, hi as u8)
}

/// MedianSmooth: removes salt-and-pepper noise with a square median window.
pub fn median(image: &mut Image, radius: usize) {
    let (w, h, r) = (image.width, image.height, radius.max(1) as isize);
    let src = image.pixels.clone();
    let mut window = [Vec::new(), Vec::new(), Vec::new()];
    for y in 0..h as isize {
        for x in 0..w as isize {
            window.iter_mut().for_each(Vec::clear);
            for dy in -r..=r {
                let yy = (y + dy).clamp(0, h as isize - 1) as usize;
                for dx in -r..=r {
                    let p = src[yy * w + (x + dx).clamp(0, w as isize - 1) as usize];
                    window[0].push((p >> 16 & 0xff) as u8);
                    window[1].push((p >> 8 & 0xff) as u8);
                    window[2].push((p & 0xff) as u8);
                }
            }
            let mid = window[0].len() / 2;
            let m: [u32; 3] = std::array::from_fn(|c| *window[c].select_nth_unstable(mid).1 as u32);
            image.pixels[y as usize * w + x as usize] = m[0] << 16 | m[1] << 8 | m[2];
        }
    }
}

fn kernel(sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil().max(1.0) as isize;
    let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    k.into_iter().map(|v| v / sum).collect()
}

/// Separable Gaussian blur of one plane, edges clamped.
fn blur(plane: &[f32], w: usize, h: usize, sigma: f32) -> Plane {
    if sigma <= 0.0 || plane.is_empty() {
        return plane.to_vec();
    }
    let k = kernel(sigma);
    let r = (k.len() / 2) as isize;
    let mut tmp = vec![0.0; plane.len()];
    for y in 0..h {
        let row = &plane[y * w..(y + 1) * w];
        for x in 0..w {
            tmp[y * w + x] = k.iter().enumerate().map(|(i, kv)| kv * row[(x as isize + i as isize - r).clamp(0, w as isize - 1) as usize]).sum();
        }
    }
    let mut out = vec![0.0; plane.len()];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = k.iter().enumerate().map(|(i, kv)| kv * tmp[(y as isize + i as isize - r).clamp(0, h as isize - 1) as usize * w + x]).sum();
        }
    }
    out
}

/// Smoothness: Gaussian blur with the given radius in pixels (sigma).
pub fn smooth(image: &mut Image, sigma: f32) {
    let (w, h) = (image.width, image.height);
    per_plane(image, |p| blur(&p, w, h, sigma));
}

/// Sharpness: unsharp mask. `amount` 1.0 adds the full detail layer once more.
pub fn sharpen(image: &mut Image, sigma: f32, amount: f32) {
    let (w, h) = (image.width, image.height);
    per_plane(image, |p| {
        let soft = blur(&p, w, h, sigma);
        p.iter().zip(&soft).map(|(v, s)| v + amount * (v - s)).collect()
    });
}

/// MultScaleWLN: multi-scale detail enhancement. The image is split into detail
/// bands at doubling blur radii plus a smooth base; each band is boosted by
/// `gain` (finer bands a little more) and the result is stretched back to 0..255.
/// It brings out enamel edges, root canals and bone trabeculae at once.
pub fn enhance(image: &mut Image, levels: usize, gain: f32) {
    let (w, h) = (image.width, image.height);
    per_plane(image, |p| {
        let mut out = vec![0.0; p.len()];
        let mut current = p;
        for level in 0..levels.max(1) {
            let next = blur(&current, w, h, 2f32.powi(level as i32));
            let boost = 1.0 + gain * (levels - level) as f32 / levels as f32;
            for i in 0..out.len() {
                out[i] += (current[i] - next[i]) * boost;
            }
            current = next;
        }
        out.iter().zip(&current).map(|(d, b)| d + b).collect::<Plane>()
    });
    auto_levels(image, 0.1);
}

/// MatTransfor: resamples the image through a 3×3 affine matrix that maps output
/// pixel coordinates (x, y, 1) to source coordinates, with bilinear filtering.
/// Pixels that land outside the source become black.
pub fn transform(image: &mut Image, m: [[f32; 3]; 3]) {
    let (w, h) = (image.width, image.height);
    let src = split(image);
    let mut out: [Plane; 3] = std::array::from_fn(|_| vec![0.0; w * h]);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32, y as f32);
            let sx = m[0][0] * fx + m[0][1] * fy + m[0][2];
            let sy = m[1][0] * fx + m[1][1] * fy + m[1][2];
            if sx < 0.0 || sy < 0.0 || sx > (w - 1) as f32 || sy > (h - 1) as f32 {
                continue;
            }
            let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            let (tx, ty) = (sx - x0 as f32, sy - y0 as f32);
            for c in 0..3 {
                let s = &src[c];
                let top = s[y0 * w + x0] * (1.0 - tx) + s[y0 * w + x1] * tx;
                let bottom = s[y1 * w + x0] * (1.0 - tx) + s[y1 * w + x1] * tx;
                out[c][y * w + x] = top * (1.0 - ty) + bottom * ty;
            }
        }
    }
    join(image, &out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(width: usize, height: usize, f: impl Fn(usize, usize) -> u32) -> Image {
        let pixels = (0..width * height).map(|i| f(i % width, i / width) * 0x010101).collect();
        Image { width, height, pixels }
    }

    fn level(p: u32) -> u32 {
        p & 0xff
    }

    #[test]
    fn statistics_and_constants() {
        let mut img = grey(4, 1, |x, _| [10, 20, 30, 40][x]);
        assert_eq!(average_value(&img), 25.0);
        assert_eq!(min_max(&img), (10, 40));
        multiply_constant(&mut img, 2.0);
        assert_eq!(img.pixels.iter().map(|&p| level(p)).collect::<Vec<_>>(), [20, 40, 60, 80]);
        set(&mut img, 7);
        assert!(img.pixels.iter().all(|&p| p == 0x070707));
    }

    #[test]
    fn normalize_and_auto_levels_stretch_to_full_range() {
        let mut img = grey(4, 1, |x, _| [50, 60, 70, 100][x]);
        normalize(&mut img);
        assert_eq!(min_max(&img), (0, 255));
        // One bright outlier among 100 pixels is ignored at 1% clipping.
        let mut img = grey(100, 1, |x, _| if x == 99 { 255 } else { 100 + x as u32 % 50 });
        let (lo, hi) = auto_levels(&mut img, 1.0);
        assert_eq!((lo, hi), (100, 149));
        assert_eq!(level(img.pixels[0]), 0);
    }

    #[test]
    fn median_removes_a_lone_hot_pixel() {
        let mut img = grey(5, 5, |x, y| if (x, y) == (2, 2) { 255 } else { 80 });
        median(&mut img, 1);
        assert!(img.pixels.iter().all(|&p| level(p) == 80));
    }

    #[test]
    fn smooth_keeps_flat_areas_and_sharpen_steepens_edges() {
        let mut flat = grey(8, 8, |_, _| 90);
        smooth(&mut flat, 1.5);
        assert!(flat.pixels.iter().all(|&p| level(p) == 90));
        let edge = grey(10, 1, |x, _| if x < 5 { 100 } else { 150 });
        let mut sharp = edge.clone();
        sharpen(&mut sharp, 1.0, 1.0);
        assert!(level(sharp.pixels[4]) < 100 && level(sharp.pixels[5]) > 150);
        let mut soft = edge.clone();
        smooth(&mut soft, 1.0);
        assert!(level(soft.pixels[4]) > 100 && level(soft.pixels[5]) < 150);
    }

    #[test]
    fn enhance_spans_the_full_range() {
        let mut img = grey(32, 32, |x, y| 100 + ((x * 7 + y * 3) % 20) as u32);
        enhance(&mut img, 3, 1.0);
        let (lo, hi) = min_max(&img);
        assert!(lo < 10 && hi > 245, "{lo} {hi}");
    }

    #[test]
    fn identity_and_shift_transforms() {
        let img = grey(4, 3, |x, y| (x * 10 + y * 50) as u32);
        let mut same = img.clone();
        transform(&mut same, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(same, img);
        let mut shifted = img.clone();
        transform(&mut shifted, [[1.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(level(shifted.pixels[0]), 10);
        assert_eq!(shifted.pixels[3], 0);
    }
}
