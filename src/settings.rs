//! Preferences from the reference Settings dialog (NanoPix.exe 0x583830,
//! five tabs: General 0x58aed0, Patient 0x58b8f0, Acquisition 0x589a50,
//! Viewer 0x58d2d0, Report 0x58c3c0). Option lists are the string tables in
//! the executable's data section. Stored as `key=value` lines in
//! `settings.txt`; language, theme and clinic keep their own files.

use std::path::Path;

/// RGBA colour.
pub type Rgba = [u8; 4];

/// Generator voltage choices (0x70a3c4).
pub const KV: [&str; 5] = ["60", "65", "70", "75", "80"];
/// Tube current for a DC generator (0x70a3d8).
pub const MA_DC: [&str; 6] = ["<=0.2", "0.5", "1.0", "2.0", "3.0", ">3.0"];
/// Tube current for an AC generator (0x70a3f0).
pub const MA_AC: [&str; 3] = ["<4.0", "4.0~7.0", ">7.0"];
/// Viewer image layouts (0x70a47c).
pub const LAYOUTS: [&str; 4] = ["1x1", "1x2", "2x1", "2x2"];
/// Magnifier window sizes (0x70a48c) and ratios (0x70a498).
pub const MAGNIFIER_SIZES: [u32; 3] = [100, 150, 200];
pub const MAGNIFIER_RATIOS: [f32; 3] = [1.5, 2.0, 2.5];
/// Grid spacing in millimetres (0x70a4a4).
pub const GRID_MM: [u32; 4] = [1, 2, 5, 10];
/// Grid opacity (0x70a4c8).
pub const GRID_OPACITY: [u32; 5] = [10, 20, 30, 40, 50];
/// Line thickness in pixels (0x70a4b4, 0x70a408).
pub const THICKNESS: [u32; 5] = [1, 2, 3, 4, 5];
/// Report text sizes in points (0x70a44c).
pub const FONT_SIZES: [f32; 7] = [9.0, 10.5, 12.0, 14.0, 15.0, 16.0, 18.0];
/// Days covered by the Recently Acquired / Printed searches.
pub const RECENT_DAYS: [u32; 3] = [2, 3, 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    Small,
    Medium,
    Big,
}

impl IconSize {
    pub const ALL: [IconSize; 3] = [IconSize::Small, IconSize::Medium, IconSize::Big];
    /// UI zoom used for each size.
    pub fn zoom(self) -> f32 {
        match self {
            IconSize::Small => 0.9,
            IconSize::Medium => 1.0,
            IconSize::Big => 1.15,
        }
    }
    fn key(self) -> &'static str {
        match self {
            IconSize::Small => "small",
            IconSize::Medium => "medium",
            IconSize::Big => "big",
        }
    }
}

/// A drawn annotation's style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    pub color: Rgba,
    pub thickness: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    // General
    pub icon_size: IconSize,
    // Patient
    pub auto_chart_number: bool,
    /// Show names as "last first" instead of "first last".
    pub last_name_first: bool,
    pub recent_acquired_days: u32,
    pub recent_printed_days: u32,
    // Acquisition
    /// AC generator (else DC). The reference resets kV and mA when this changes.
    pub generator_ac: bool,
    pub kv: usize,
    pub ma: usize,
    /// Millimetres per pixel used when an image does not state its pixel
    /// spacing (the reference's "caliValue", 0.005..=1.0).
    pub calibration_mm: f64,
    // Viewer
    pub fullscreen: bool,
    pub layout: usize,
    pub magnifier_size: usize,
    pub magnifier_ratio: usize,
    pub capture_border: Rgba,
    pub grid_mm: usize,
    pub grid: Stroke,
    pub grid_opacity: usize,
    pub length: Stroke,
    pub length_unit: bool,
    pub angle: Stroke,
    pub angle_unit: bool,
    pub draw: Stroke,
    pub line: Stroke,
    pub rectangle: Stroke,
    pub ellipse: Stroke,
    pub memo_outer: Rgba,
    pub memo_text: Rgba,
    pub memo_presets: [String; 7],
    // Report
    pub border: Stroke,
    pub font_size: usize,
    pub text_color: Rgba,
}

const YELLOW: Rgba = [0xff, 0xd2, 0x3f, 0xff];
const CYAN: Rgba = [0x3f, 0xd2, 0xff, 0xff];

impl Default for Settings {
    fn default() -> Self {
        let s = |color| Stroke { color, thickness: 2 };
        Self {
            icon_size: IconSize::Medium,
            auto_chart_number: false,
            last_name_first: false,
            recent_acquired_days: 3,
            recent_printed_days: 3,
            generator_ac: false,
            kv: 2,
            ma: 3,
            calibration_mm: 0.02,
            fullscreen: false,
            layout: 0,
            magnifier_size: 1,
            magnifier_ratio: 1,
            capture_border: [0xff, 0xff, 0xff, 0xff],
            grid_mm: 2,
            grid: Stroke { color: [0xff, 0xff, 0xff, 0xff], thickness: 1 },
            grid_opacity: 1,
            length: s(YELLOW),
            length_unit: true,
            angle: s(CYAN),
            angle_unit: true,
            draw: s([0xff, 0x40, 0x40, 0xff]),
            line: s([0x40, 0xff, 0x80, 0xff]),
            rectangle: s([0x40, 0x9c, 0xff, 0xff]),
            ellipse: s([0xff, 0x80, 0xff, 0xff]),
            memo_outer: [0x20, 0x20, 0x20, 0xc0],
            memo_text: [0xff, 0xff, 0xff, 0xff],
            memo_presets: Default::default(),
            border: Stroke { color: [0x40, 0x40, 0x40, 0xff], thickness: 1 },
            font_size: 2,
            text_color: [0, 0, 0, 0xff],
        }
    }
}

fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
}

fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.strip_prefix('#')?;
    if s.len() != 8 || !s.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?, b(6)?])
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\r', "").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Settings {
    /// Millimetres per pixel the ruler uses: the image's own spacing, else the calibration value.
    pub fn mm_per_pixel(&self, from_image: Option<f64>) -> f64 {
        from_image.unwrap_or(self.calibration_mm)
    }

    /// The mA choices for the selected generator type.
    pub fn ma_choices(&self) -> &'static [&'static str] {
        if self.generator_ac { &MA_AC } else { &MA_DC }
    }

    /// Switching generator type resets kV and mA, as the reference does
    /// (DC → kV 70, mA 2.0; AC → kV 70, mA 4.0~7.0).
    pub fn set_generator_ac(&mut self, ac: bool) {
        self.generator_ac = ac;
        self.kv = 2;
        self.ma = if ac { 1 } else { 3 };
    }

    pub fn to_text(&self) -> String {
        let mut lines: Vec<(String, String)> = Vec::new();
        let mut put = |k: &str, v: String| lines.push((k.to_string(), v));
        put("icon_size", self.icon_size.key().into());
        put("auto_chart_number", self.auto_chart_number.to_string());
        put("last_name_first", self.last_name_first.to_string());
        put("recent_acquired_days", self.recent_acquired_days.to_string());
        put("recent_printed_days", self.recent_printed_days.to_string());
        put("generator", if self.generator_ac { "ac" } else { "dc" }.into());
        put("kv", self.kv.to_string());
        put("ma", self.ma.to_string());
        put("calibration_mm", self.calibration_mm.to_string());
        put("fullscreen", self.fullscreen.to_string());
        put("layout", self.layout.to_string());
        put("magnifier_size", self.magnifier_size.to_string());
        put("magnifier_ratio", self.magnifier_ratio.to_string());
        put("capture_border", hex(self.capture_border));
        put("grid_mm", self.grid_mm.to_string());
        put("grid_opacity", self.grid_opacity.to_string());
        for (name, s) in self.strokes() {
            put(&format!("{name}_color"), hex(s.color));
            put(&format!("{name}_thickness"), s.thickness.to_string());
        }
        put("length_unit", self.length_unit.to_string());
        put("angle_unit", self.angle_unit.to_string());
        put("memo_outer", hex(self.memo_outer));
        put("memo_text", hex(self.memo_text));
        for (i, p) in self.memo_presets.iter().enumerate() {
            put(&format!("memo_preset{}", i + 1), escape(p));
        }
        put("font_size", self.font_size.to_string());
        put("text_color", hex(self.text_color));
        lines.into_iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    fn strokes(&self) -> [(&'static str, Stroke); 8] {
        [
            ("grid", self.grid),
            ("length", self.length),
            ("angle", self.angle),
            ("draw", self.draw),
            ("line", self.line),
            ("rectangle", self.rectangle),
            ("ellipse", self.ellipse),
            ("border", self.border),
        ]
    }

    fn stroke_mut(&mut self, name: &str) -> Option<&mut Stroke> {
        Some(match name {
            "grid" => &mut self.grid,
            "length" => &mut self.length,
            "angle" => &mut self.angle,
            "draw" => &mut self.draw,
            "line" => &mut self.line,
            "rectangle" => &mut self.rectangle,
            "ellipse" => &mut self.ellipse,
            "border" => &mut self.border,
            _ => return None,
        })
    }

    /// Unknown keys and invalid values keep their defaults; indexes are
    /// clamped to their option lists.
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
            let (k, v) = (k.trim(), v.trim_end_matches('\r'));
            let flag = |b: &mut bool| {
                if let Ok(x) = v.parse() {
                    *b = x;
                }
            };
            let index = |i: &mut usize, len: usize| {
                if let Ok(x) = v.parse::<usize>() {
                    *i = x.min(len - 1);
                }
            };
            let color = |c: &mut Rgba| {
                if let Some(x) = parse_hex(v) {
                    *c = x;
                }
            };
            match k {
                "icon_size" => {
                    if let Some(x) = IconSize::ALL.into_iter().find(|i| i.key() == v) {
                        s.icon_size = x;
                    }
                }
                "auto_chart_number" => flag(&mut s.auto_chart_number),
                "last_name_first" => flag(&mut s.last_name_first),
                "recent_acquired_days" => s.recent_acquired_days = v.parse().ok().filter(|d| RECENT_DAYS.contains(d)).unwrap_or(s.recent_acquired_days),
                "recent_printed_days" => s.recent_printed_days = v.parse().ok().filter(|d| RECENT_DAYS.contains(d)).unwrap_or(s.recent_printed_days),
                "generator" => s.generator_ac = v == "ac",
                "kv" => index(&mut s.kv, KV.len()),
                "ma" => index(&mut s.ma, MA_DC.len()),
                "calibration_mm" => {
                    if let Ok(x) = v.parse::<f64>() {
                        s.calibration_mm = x.clamp(0.005, 1.0);
                    }
                }
                "fullscreen" => flag(&mut s.fullscreen),
                "layout" => index(&mut s.layout, LAYOUTS.len()),
                "magnifier_size" => index(&mut s.magnifier_size, MAGNIFIER_SIZES.len()),
                "magnifier_ratio" => index(&mut s.magnifier_ratio, MAGNIFIER_RATIOS.len()),
                "capture_border" => color(&mut s.capture_border),
                "grid_mm" => index(&mut s.grid_mm, GRID_MM.len()),
                "grid_opacity" => index(&mut s.grid_opacity, GRID_OPACITY.len()),
                "length_unit" => flag(&mut s.length_unit),
                "angle_unit" => flag(&mut s.angle_unit),
                "memo_outer" => color(&mut s.memo_outer),
                "memo_text" => color(&mut s.memo_text),
                "font_size" => index(&mut s.font_size, FONT_SIZES.len()),
                "text_color" => color(&mut s.text_color),
                _ => {
                    if let Some(n) = k.strip_prefix("memo_preset").and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=7).contains(n)) {
                        s.memo_presets[n - 1] = unescape(v);
                    } else if let Some((name, field)) = k.rsplit_once('_') {
                        if let Some(st) = s.stroke_mut(name) {
                            match field {
                                "color" => color(&mut st.color),
                                "thickness" => {
                                    if let Ok(t) = v.parse::<u32>() {
                                        st.thickness = t.clamp(1, 5);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        s.ma = s.ma.min(s.ma_choices().len() - 1);
        s
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map(|t| Self::from_text(&t)).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.to_text()).map_err(|e| format!("Cannot save the settings: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_every_field() {
        let mut s = Settings::default();
        s.icon_size = IconSize::Big;
        s.auto_chart_number = true;
        s.recent_printed_days = 4;
        s.set_generator_ac(true);
        s.calibration_mm = 0.035;
        s.layout = 3;
        s.length = Stroke { color: [1, 2, 3, 4], thickness: 5 };
        s.angle_unit = false;
        s.memo_presets[6] = "Caries\nmesial".into();
        s.font_size = 6;
        assert_eq!(Settings::from_text(&s.to_text()), s);
    }

    #[test]
    fn bad_values_fall_back_or_clamp() {
        let s = Settings::from_text("kv=99\ncalibration_mm=7\nlength_color=red\nrecent_acquired_days=9\ngenerator=ac\nma=5\nborder_thickness=0\n");
        assert_eq!(s.kv, KV.len() - 1);
        assert_eq!(s.calibration_mm, 1.0);
        assert_eq!(s.length.color, Settings::default().length.color);
        assert_eq!(s.recent_acquired_days, 3);
        assert_eq!(s.ma, MA_AC.len() - 1, "mA is clamped to the AC list");
        assert_eq!(s.border.thickness, 1);
        assert_eq!(s.mm_per_pixel(None), 1.0);
        assert_eq!(s.mm_per_pixel(Some(0.02)), 0.02);
    }
}
