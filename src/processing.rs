//! The ImageProcessing library of the reference CoreLibrary.dll, ported to Rust.
//!
//! The algorithms below were mapped from the disassembly of CoreLibrary.dll
//! 1.1.1.9 and the call sites in NanoPix.exe (see PROGRESS.md, "ImageProcessing
//! mapping"). The reference works on 16-bit grey pixels (0..65535); LumoDent keeps
//! 8-bit channels, so every constant tied to the pixel range is scaled to 0..255
//! (`MAX`). Rounding follows the reference: results are clamped, then truncated.
//!
//! Mapped from the binary: Smoothness, Sharpness, MedianSmooth, MultScaleWLN,
//! BrightnessAndContrast (see `crate::image::levels`), AutoBrightnessAndContrast,
//! Invert, GetMinMaxIntegerValue, ImageNormalizeEx and MatTransfor (the last two
//! decompiled with Ghidra 12.2 through REA). Not mapped yet, rewrite's own math:
//! Set, MultiplyConstant, GetAverageValue.

use crate::image::Image;

type Plane = Vec<f32>;

/// Top of the pixel range (65535 in the reference).
const MAX: f32 = 255.0;

fn split(image: &Image) -> [Plane; 3] {
    std::array::from_fn(|c| {
        let shift = 16 - 8 * c as u32;
        image.pixels.iter().map(|p| (p >> shift & 0xff) as f32).collect()
    })
}

/// Clamp to 0..MAX, then truncate toward zero, as the reference does.
fn store(v: f32) -> u32 {
    v.clamp(0.0, MAX) as u32
}

fn join(image: &mut Image, planes: &[Plane; 3]) {
    for (i, p) in image.pixels.iter_mut().enumerate() {
        *p = store(planes[0][i]) << 16 | store(planes[1][i]) << 8 | store(planes[2][i]);
    }
}

fn is_grey(image: &Image) -> bool {
    image.pixels.iter().all(|&p| p >> 16 & 0xff == p & 0xff && p >> 8 & 0xff == p & 0xff)
}

/// Runs `f` on each channel plane. The reference only handles grey images; a grey
/// image is processed once and copied to all channels, colour images per channel.
fn per_plane(image: &mut Image, f: impl Fn(Plane) -> Plane) {
    if is_grey(image) {
        let out = f(image.pixels.iter().map(|p| (p & 0xff) as f32).collect());
        for (p, v) in image.pixels.iter_mut().zip(out) {
            *p = store(v) * 0x010101;
        }
    } else {
        let planes = split(image).map(f);
        join(image, &planes);
    }
}

fn map(image: &mut Image, f: impl Fn(f32) -> f32) {
    let curve: [u32; 256] = std::array::from_fn(|v| store(f(v as f32)));
    for p in &mut image.pixels {
        *p = curve[(*p >> 16 & 0xff) as usize] << 16 | curve[(*p >> 8 & 0xff) as usize] << 8 | curve[(*p & 0xff) as usize];
    }
}

fn luma(p: u32) -> u32 {
    ((p >> 16 & 0xff) * 77 + (p >> 8 & 0xff) * 150 + (p & 0xff) * 29) >> 8
}

/// Mirror index without repeating the edge pixel (OpenCV's BORDER_REFLECT_101),
/// the padding the reference builds before every window filter.
fn reflect101(i: isize, n: usize) -> usize {
    if n == 1 {
        return 0;
    }
    let (n, mut i) = (n as isize, i);
    loop {
        if i < 0 {
            i = -i;
        } else if i >= n {
            i = 2 * n - 2 - i;
        } else {
            return i as usize;
        }
    }
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

/// GetMinMaxIntegerValue: darkest and brightest channel value.
pub fn min_max(image: &Image) -> (u8, u8) {
    image.pixels.iter().fold((255, 0), |(lo, hi), &p| {
        let c = [(p >> 16 & 0xff) as u8, (p >> 8 & 0xff) as u8, (p & 0xff) as u8];
        (lo.min(*c.iter().min().unwrap()), hi.max(*c.iter().max().unwrap()))
    })
}

/// ImageNormalizeEx: `(v − min)·(out_max − out_min)/(max − min)`, clamped to
/// out_min..out_max and truncated. As in the reference, out_min only bounds the
/// result; it is not added as an offset. The default range is 0..255.
pub fn normalize_to(image: &mut Image, out_min: f64, out_max: f64) {
    let (lo, hi) = min_max(image);
    if hi == lo {
        return; // The reference divides by zero here.
    }
    let (lo, hi) = (lo as f64, hi as f64);
    let scale = (out_max - out_min) / (hi - lo);
    let curve: [u32; 256] = std::array::from_fn(|v| ((v as f64 - lo) * scale).min(out_max).max(out_min).clamp(0.0, MAX as f64) as u32);
    for p in &mut image.pixels {
        *p = curve[(*p >> 16 & 0xff) as usize] << 16 | curve[(*p >> 8 & 0xff) as usize] << 8 | curve[(*p & 0xff) as usize];
    }
}

/// ImageNormalizeEx with its default output range.
pub fn normalize(image: &mut Image) {
    normalize_to(image, 0.0, MAX as f64);
}

/// Invert as the viewer calls it: each value becomes `max − value`, where max is
/// the brightest value in the image (not the top of the range).
pub fn invert(image: &mut Image) {
    let (_, max) = min_max(image);
    let max = max as u32;
    for p in &mut image.pixels {
        let c = |shift: u32| (max - (*p >> shift & 0xff).min(max)) << shift;
        *p = c(16) | c(8) | c(0);
    }
}

/// The (2r+1)² Gaussian the reference builds: exp(−(dx² + dy²) / 2σ²), optionally
/// scaled so the weights sum to one.
fn gaussian(radius: usize, sigma: f32, normalize: bool) -> Vec<f32> {
    let (r, side) = (radius as i32, 2 * radius + 1);
    let scale = -1.0 / (2.0 * sigma * sigma);
    let mut k: Vec<f32> = (0..side * side)
        .map(|i| {
            let (dy, dx) = ((i / side) as i32 - r, (i % side) as i32 - r);
            (((dx * dx) as f64 + (dy * dy) as f64) * scale as f64).exp() as f32
        })
        .collect();
    if normalize {
        let inv = 1.0 / k.iter().sum::<f32>();
        k.iter_mut().for_each(|v| *v *= inv);
    }
    k
}

/// Square-kernel convolution with reflect-101 borders.
fn convolve(plane: &[f32], w: usize, h: usize, kernel: &[f32], radius: usize) -> Plane {
    let (r, side) = (radius as isize, 2 * radius + 1);
    let mut out = vec![0.0; plane.len()];
    for y in 0..h {
        let rows: Vec<usize> = (-r..=r).map(|d| reflect101(y as isize + d, h) * w).collect();
        for x in 0..w {
            let mut sum = 0.0;
            for (ky, row) in rows.iter().enumerate() {
                for kx in 0..side {
                    sum += kernel[ky * side + kx] * plane[row + reflect101(x as isize + kx as isize - r, w)];
                }
            }
            out[y * w + x] = sum;
        }
    }
    out
}

/// Smoothness: Gaussian blur with a (2r+1)² kernel. The viewer uses r = 2, σ = 4.
pub fn smooth(image: &mut Image, radius: usize, sigma: f32) {
    let (w, h) = (image.width, image.height);
    let k = gaussian(radius, sigma, true);
    per_plane(image, |p| convolve(&p, w, h, &k, radius));
}

/// Sharpness: unsharp mask with an unnormalised Gaussian K (weights summing to S):
/// `v·(1 + a·S) − a·(K∗v)`. The viewer uses r = 1, σ = 1 and a = 0.6 × level.
pub fn sharpen(image: &mut Image, radius: usize, amount: f32, sigma: f32) {
    let (w, h) = (image.width, image.height);
    let mut k = gaussian(radius, sigma, false);
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v *= amount);
    let centre = sum * amount + 1.0;
    per_plane(image, |p| {
        let soft = convolve(&p, w, h, &k, radius);
        p.iter().zip(&soft).map(|(v, s)| v * centre - s).collect()
    });
}

/// MedianSmooth: median of the (2r+1)² window, reflect-101 borders.
pub fn median(image: &mut Image, radius: usize) {
    let (w, h, r) = (image.width, image.height, radius.max(1) as isize);
    per_plane(image, |p| {
        let mut window = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
        let mut out = vec![0.0; p.len()];
        for y in 0..h as isize {
            for x in 0..w as isize {
                window.clear();
                for dy in -r..=r {
                    let row = reflect101(y + dy, h) * w;
                    for dx in -r..=r {
                        window.push(p[row + reflect101(x + dx, w)] as u16);
                    }
                }
                let mid = window.len() / 2;
                out[y as usize * w + x as usize] = *window.select_nth_unstable(mid).1 as f32;
            }
        }
        out
    });
}

/// MultScaleWLN ("multiple window width/level"): a tone curve averaged from `n`
/// linear windows. Window k maps the input range from
/// `min + k(a − min)/n` to `max + k(b − max)/n` onto the output range c..d, where
/// min/max are the image's own extremes; below the window is c, above it d.
/// After acquisition the reference runs it with n = 10, a = 10000, b = c..d = 0..65535.
pub fn window_levels(image: &mut Image, n: u32, a: f64, b: f64, c: f64, d: f64) {
    if (a - c).abs() < 1e-6 && (b - d).abs() < 1e-6 {
        return;
    }
    let n = n.max(1);
    let (lo, hi) = min_max(image);
    let (lo, hi) = (lo as f64, hi as f64);
    let (step_lo, step_hi) = ((a - lo) / n as f64, (b - hi) / n as f64);
    let mut lut = [0.0f64; 256];
    for k in 1..=n {
        let x1 = lo + k as f64 * step_lo;
        let x2 = hi + k as f64 * step_hi;
        if (x1 - x2).abs() < 1e-6 {
            let v = ((c + d) / 2.0).clamp(0.0, MAX as f64);
            lut.iter_mut().for_each(|l| *l += v / n as f64);
        } else {
            let slope = (d - c) / (x2 - x1);
            let offset = c - slope * x1;
            for (i, l) in lut.iter_mut().enumerate() {
                let i = i as f64;
                let v = if i < x1 { c } else if i <= x2 { i * slope + offset } else { d };
                *l += v / n as f64;
            }
        }
    }
    let curve: [u32; 256] = std::array::from_fn(|i| (lut[i] as i64).clamp(0, MAX as i64) as u32);
    for p in &mut image.pixels {
        *p = curve[(*p >> 16 & 0xff) as usize] << 16 | curve[(*p >> 8 & 0xff) as usize] << 8 | curve[(*p & 0xff) as usize];
    }
}

/// The post-acquisition look of the reference: MultScaleWLN with n = 10, a = 10000
/// of 65535, output 0..max.
pub fn enhance(image: &mut Image) {
    window_levels(image, 10, 10000.0 * MAX as f64 / 65535.0, MAX as f64, 0.0, MAX as f64);
}

/// The input window of BrightnessAndContrast for slider values −100..100, before
/// truncation: centre 127.5 − 0.64·b, width 256 / e^(c/100).
pub fn tone_window(brightness: i32, contrast: i32) -> (f32, f32) {
    let centre = -(brightness as f32) * ((MAX + 1.0) / 400.0) + MAX / 2.0;
    let width = (MAX + 1.0) / (contrast as f32 * 0.01).exp();
    (centre - width / 2.0 + 0.5, centre + width - (width / 2.0 + 0.5))
}

/// AutoBrightnessAndContrast: returns new brightness/contrast slider values (the
/// pixels are not touched). Starting from the current window, it trims the
/// histogram tail on whichever side has fewer pixels outside, until `1 − ratio`
/// of all pixels are left out, then turns the window back into slider values.
/// Sliders already at a limit reset to 0, so pressing Auto again undoes it. The
/// viewer uses ratio 0.97 and limits of ±100.
pub fn auto_brightness_contrast(image: &Image, brightness: i32, contrast: i32, limit: i32, ratio: f32) -> (i32, i32) {
    if (brightness.abs() - limit).abs() <= 0 || (contrast.abs() - limit).abs() <= 0 {
        return (0, 0);
    }
    let (lo, hi) = tone_window(brightness, contrast);
    let (mut lo, mut hi) = (lo as i32, hi as i32);
    if lo >= hi - 1 {
        let (a, b) = min_max(image);
        (lo, hi) = (a as i32, b as i32);
    }
    let mut hist = [0i64; 256];
    for &p in &image.pixels {
        hist[luma(p) as usize] += 1;
    }
    let at = |i: i32| if (0..256).contains(&i) { hist[i as usize] } else { 0 };
    let inside: i64 = (lo..=hi).map(at).sum();
    let target = (inside as f64 - (1.0 - ratio as f64) * image.pixels.len() as f64) as i64;
    let mut remaining = inside;
    let mut below: i64 = (0..lo).map(at).sum();
    let mut above: i64 = (hi + 1..256).map(at).sum();
    while remaining >= target {
        let (low, high) = (at(lo), at(hi));
        below += low;
        above += high;
        if below < above {
            remaining -= low;
            above -= high;
            lo += 1;
        } else {
            remaining -= high;
            below -= low;
            hi -= 1;
        }
        if lo >= hi {
            break;
        }
    }
    // Move each slider to the new value, by at least one step.
    let toward = |current: i32, wanted: f32| {
        let delta = wanted - current as f32;
        let step = if delta >= 0.0 { delta.max(1.0) } else { delta.min(-1.0) };
        current.saturating_add(step as i32)
    };
    let wanted_contrast = ((MAX + 1.0) / (hi - lo) as f32).ln() * 100.0 + 0.5;
    let wanted_brightness = -(((hi + lo) as f32 / 2.0) - MAX / 2.0) / ((MAX + 1.0) / 400.0);
    let contrast = toward(contrast, wanted_contrast).clamp(-limit, limit);
    let brightness = toward(brightness, wanted_brightness).clamp(-limit, limit);
    (brightness, contrast)
}

/// MatTransfor: moves each source pixel (x, y) to
/// `(a·x + b·y + c, d·x + e·y + f)` with an integer matrix `[[a, b, c], [d, e, f]]`,
/// the rotations and flips NanoPix builds (GetFlipTransMatrix). The output is
/// `|a·w + b·h|` × `|d·w + e·h|`. Pixels mapped outside are dropped (the reference
/// writes past its buffer there).
pub fn transform(image: &mut Image, m: [[i64; 3]; 2]) {
    let (w, h) = (image.width as i64, image.height as i64);
    let ow = (m[0][0] * w + m[0][1] * h).abs();
    let oh = (m[1][0] * w + m[1][1] * h).abs();
    let mut out = vec![0u32; (ow * oh) as usize];
    for y in 0..h {
        for x in 0..w {
            let tx = m[0][0] * x + m[0][1] * y + m[0][2];
            let ty = m[1][0] * x + m[1][1] * y + m[1][2];
            if (0..ow).contains(&tx) && (0..oh).contains(&ty) {
                out[(ty * ow + tx) as usize] = image.pixels[(y * w + x) as usize];
            }
        }
    }
    (image.width, image.height, image.pixels) = (ow as usize, oh as usize, out);
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
    fn reflect_101_mirrors_without_repeating_the_edge() {
        let got: Vec<usize> = (-3..8).map(|i| reflect101(i, 5)).collect();
        assert_eq!(got, [3, 2, 1, 0, 1, 2, 3, 4, 3, 2, 1]);
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
    fn invert_is_relative_to_the_brightest_pixel() {
        let mut img = grey(3, 1, |x, _| [10, 50, 200][x]);
        invert(&mut img);
        assert_eq!(img.pixels.iter().map(|&p| level(p)).collect::<Vec<_>>(), [190, 150, 0]);
    }

    #[test]
    fn normalize_stretches_to_full_range() {
        let mut img = grey(4, 1, |x, _| [50, 60, 70, 100][x]);
        normalize(&mut img);
        // 50 × (255/50) lands a hair under 255 in doubles; the reference truncates.
        assert_eq!(min_max(&img), (0, 254));
    }

    #[test]
    fn gaussian_kernel_matches_the_reference_formula() {
        let k = gaussian(1, 1.0, false);
        assert_eq!(k[4], 1.0);
        assert!((k[1] - (-0.5f32).exp()).abs() < 1e-6 && (k[0] - (-1.0f32).exp()).abs() < 1e-6);
        let n = gaussian(2, 4.0, true);
        assert!((n.iter().sum::<f32>() - 1.0).abs() < 1e-5);
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
        smooth(&mut flat, 2, 4.0);
        // Normalised float weights can sum a hair under one; the reference truncates.
        assert!(flat.pixels.iter().all(|&p| (89..=90).contains(&level(p))));
        let mut flat = grey(8, 8, |_, _| 90);
        sharpen(&mut flat, 1, 0.6, 1.0);
        assert!(flat.pixels.iter().all(|&p| (89..=90).contains(&level(p))));
        let edge = grey(10, 3, |x, _| if x < 5 { 100 } else { 150 });
        let mut sharp = edge.clone();
        sharpen(&mut sharp, 1, 0.6, 1.0);
        assert!(level(sharp.pixels[14]) < 100 && level(sharp.pixels[15]) > 150);
        let mut soft = edge.clone();
        smooth(&mut soft, 2, 4.0);
        assert!(level(soft.pixels[14]) > 100 && level(soft.pixels[15]) < 150);
    }

    #[test]
    fn window_levels_single_window_is_a_linear_stretch() {
        // One window, input 50..150 onto 0..255.
        let mut img = grey(3, 1, |x, _| [50, 100, 150][x]);
        window_levels(&mut img, 1, 50.0, 150.0, 0.0, 255.0);
        assert_eq!(img.pixels.iter().map(|&p| level(p)).collect::<Vec<_>>(), [0, 127, 255]);
        // Same input and output range is left alone.
        let mut same = grey(2, 1, |x, _| [3, 9][x]);
        window_levels(&mut same, 10, 0.0, 255.0, 0.0, 255.0);
        assert_eq!(same.pixels, [0x030303, 0x090909]);
    }

    #[test]
    fn enhance_darkens_the_low_range_and_keeps_the_top() {
        let mut img = grey(256, 1, |x, _| x as u32);
        enhance(&mut img);
        assert_eq!(level(img.pixels[0]), 0);
        assert_eq!(level(img.pixels[255]), 255);
        assert!(level(img.pixels[60]) < 60, "{}", level(img.pixels[60]));
    }

    #[test]
    fn tone_window_is_identity_at_zero() {
        assert_eq!(tone_window(0, 0), (0.0, 255.0));
    }

    #[test]
    fn auto_brightness_contrast_narrows_to_the_used_range() {
        // A dim, low-contrast image: values 60..100.
        let img = grey(41, 10, |x, _| 60 + x as u32);
        let (b, c) = auto_brightness_contrast(&img, 0, 0, 100, 0.97);
        // The kept range is ~40 wide, far narrower than the 256 contrast 100 allows.
        assert_eq!(c, 100);
        // Its middle, 80, sits 47.5 below mid-grey: brightness ≈ 47.5 / 0.64.
        assert!((70..=78).contains(&b), "brightness {b}");
        // At a limit it resets.
        assert_eq!(auto_brightness_contrast(&img, 100, 5, 100, 0.97), (0, 0));
    }

    #[test]
    fn integer_matrix_transforms_match_flips_and_rotation() {
        let img = grey(4, 3, |x, y| (x * 10 + y * 50) as u32);
        let mut same = img.clone();
        transform(&mut same, [[1, 0, 0], [0, 1, 0]]);
        assert_eq!(same, img);
        let mut flipped = img.clone();
        transform(&mut flipped, [[-1, 0, 3], [0, 1, 0]]);
        let mut expected = img.clone();
        expected.flip_horizontal();
        assert_eq!(flipped, expected);
        // Clockwise: (x, y) → (h−1−y, x), output h × w.
        let mut turned = img.clone();
        transform(&mut turned, [[0, -1, 2], [1, 0, 0]]);
        let mut expected = img.clone();
        expected.rotate_clockwise();
        assert_eq!(turned, expected);
    }

    #[test]
    fn normalize_does_not_offset_by_the_output_minimum() {
        let mut img = grey(3, 1, |x, _| [100, 150, 200][x]);
        normalize_to(&mut img, 50.0, 250.0);
        // (v − 100)·2 = 0, 100, 200, then clamped to 50..250.
        assert_eq!(img.pixels.iter().map(|&p| level(p)).collect::<Vec<_>>(), [50, 100, 200]);
    }
}
