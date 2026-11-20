//! The LumoDent mark: a molar lit from above (white fading to the accent
//! blue), on a dark disc for the window and file icons. One outline feeds the
//! window icon, the in-app logo, the .exe icon (build.rs includes this file)
//! and packaging/linux/lumodent.svg.

/// The tooth outline in unit coordinates (y down): two cusps, a waist, two roots.
pub const TOOTH: [(f32, f32); 23] = [
    (0.20, 0.30),
    (0.21, 0.17),
    (0.30, 0.09),
    (0.41, 0.10),
    (0.50, 0.17),
    (0.59, 0.10),
    (0.70, 0.09),
    (0.79, 0.17),
    (0.80, 0.30),
    (0.77, 0.47),
    (0.73, 0.69),
    (0.69, 0.87),
    (0.64, 0.92),
    (0.60, 0.87),
    (0.57, 0.67),
    (0.50, 0.58),
    (0.43, 0.67),
    (0.40, 0.87),
    (0.36, 0.92),
    (0.31, 0.87),
    (0.27, 0.69),
    (0.23, 0.47),
    (0.20, 0.30),
];

pub const DISC: [u8; 3] = [0x2c, 0x34, 0x3e];
pub const TOP: [u8; 3] = [0xff, 0xff, 0xff];
pub const BOTTOM: [u8; 3] = [0x8f, 0xb8, 0xe0];

/// Even-odd point-in-polygon.
pub fn inside_tooth(x: f32, y: f32) -> bool {
    let mut inside = false;
    let n = TOOTH.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = TOOTH[i];
        let (xj, yj) = TOOTH[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// RGBA pixels of an `n`×`n` icon, 4×4 supersampled. With `disc` the tooth
/// sits on a dark circle (scaled to 80%), else on transparency.
pub fn render(n: usize, disc: bool, top: [u8; 3], bottom: [u8; 3]) -> Vec<u8> {
    let mut out = vec![0u8; n * n * 4];
    const S: usize = 4;
    let scale = if disc { 0.8 } else { 1.0 };
    for py in 0..n {
        for px in 0..n {
            let (mut tooth, mut circle) = (0usize, 0usize);
            for sy in 0..S {
                for sx in 0..S {
                    let x = (px as f32 + (sx as f32 + 0.5) / S as f32) / n as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / S as f32) / n as f32;
                    if disc && (x - 0.5).powi(2) + (y - 0.5).powi(2) <= 0.235 {
                        circle += 1;
                    }
                    let (tx, ty) = ((x - 0.5) / scale + 0.5, (y - 0.5) / scale + 0.5);
                    if inside_tooth(tx, ty) {
                        tooth += 1;
                    }
                }
            }
            let a_tooth = tooth as f32 / (S * S) as f32;
            let a_disc = if disc { circle as f32 / (S * S) as f32 } else { 0.0 };
            // Light falls from the top: white at the crown, accent at the root tips.
            let ty = (((py as f32 + 0.5) / n as f32 - 0.5) / scale + 0.5).clamp(0.08, 0.92);
            let k = ((ty - 0.08) / 0.84).clamp(0.0, 1.0).powf(1.6);
            let tint = [0, 1, 2].map(|i| top[i] as f32 + (bottom[i] as f32 - top[i] as f32) * k);
            let alpha = a_tooth + a_disc * (1.0 - a_tooth);
            let p = &mut out[(py * n + px) * 4..][..4];
            if alpha > 0.0 {
                for i in 0..3 {
                    let c = tint[i] * a_tooth + DISC[i] as f32 * a_disc * (1.0 - a_tooth);
                    p[i] = (c / alpha).round().clamp(0.0, 255.0) as u8;
                }
                p[3] = (alpha * 255.0).round() as u8;
            }
        }
    }
    out
}

/// The outline as an SVG path in a 64-unit box.
pub fn svg_path(size: f32) -> String {
    let mut d = String::new();
    for (i, (x, y)) in TOOTH.iter().enumerate() {
        d.push_str(&format!("{}{:.1} {:.1} ", if i == 0 { "M" } else { "L" }, x * size, y * size));
    }
    d.push('Z');
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_shape() {
        assert!(inside_tooth(0.5, 0.35), "crown");
        assert!(inside_tooth(0.33, 0.8) && inside_tooth(0.66, 0.8), "both roots");
        assert!(!inside_tooth(0.5, 0.75), "gap between the roots");
        assert!(!inside_tooth(0.5, 0.12), "notch between the cusps");
        let px = render(64, true, TOP, BOTTOM);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..][..4];
        assert_eq!(at(0, 0)[3], 0, "transparent corner");
        assert_eq!(&at(32, 3)[..3], &DISC, "disc");
        assert!(at(32, 22)[0] > 0xe0, "white crown");
        assert!(svg_path(64.0).starts_with("M12.8 19.2 L"));
    }
}
