//! Reading an existing NanoPix installation, so LumoDent opens the same
//! patients on a PC that already used the original software.
//!
//! NanoPix keeps its preferences in `%APPDATA%\NanoPix\NanoPix.ini` (an INI
//! file whose `[Acquisition] Patient Data Directory` names the data folder,
//! `D:\PatientData` by default) and its two SQLite databases in
//! `<data folder>\database\NanoPix.db3` and `User.db3`. Their tables are the
//! ones LumoDent already uses, so the files are opened as they are.

use crate::reports::Clinic;
use crate::settings::{Rgba, Settings, Stroke, FONT_SIZES, GRID_MM, GRID_OPACITY, KV, LAYOUTS, MAGNIFIER_RATIOS, MAGNIFIER_SIZES, MA_AC, MA_DC};
use std::path::{Path, PathBuf};

/// `section/key` → value, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct Ini(Vec<(String, String, String)>);

impl Ini {
    pub fn parse(text: &str) -> Self {
        let mut section = String::new();
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.starts_with(';') || line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_string();
            } else if let Some((k, v)) = line.split_once('=') {
                out.push((section.clone(), k.trim().to_string(), v.trim().to_string()));
            }
        }
        Ini(out)
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.0.iter().find(|(s, k, _)| s.eq_ignore_ascii_case(section) && k.eq_ignore_ascii_case(key)).map(|(_, _, v)| v.as_str())
    }
}

/// `%APPDATA%\NanoPix\NanoPix.ini` (Windows only).
pub fn ini_path() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("NanoPix").join("NanoPix.ini"))
    } else {
        None
    }
}

/// An existing NanoPix data folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Install {
    pub data: PathBuf,
    pub ini: Ini,
}

impl Install {
    /// A data folder chosen by hand (e.g. copied from another PC): it must
    /// hold database/NanoPix.db3. NanoPix's INI, when present, still supplies
    /// the preferences.
    pub fn from_folder(dir: &Path) -> Option<Self> {
        let ini = ini_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Ini::parse(&t)).unwrap_or_else(|| Ini::parse(""));
        let install = Install { data: dir.to_path_buf(), ini };
        install.main_db().is_file().then_some(install)
    }

    pub fn main_db(&self) -> PathBuf {
        self.data.join("database").join("NanoPix.db3")
    }
    pub fn user_db(&self) -> PathBuf {
        self.data.join("database").join("User.db3")
    }
}

/// Finds the installation an INI points to: its data folder must hold the
/// NanoPix database.
pub fn detect(ini_file: &Path) -> Option<Install> {
    let text = std::fs::read_to_string(ini_file).ok()?;
    let ini = Ini::parse(&text);
    let data = PathBuf::from(ini.get("Acquisition", "Patient Data Directory").filter(|d| !d.is_empty()).unwrap_or(r"D:\PatientData"));
    let install = Install { data, ini };
    install.main_db().is_file().then_some(install)
}

fn on(v: &str) -> bool {
    v.eq_ignore_ascii_case("ON")
}

/// "255,110,89,255"
fn color(v: &str) -> Option<Rgba> {
    let p: Vec<u8> = v.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
    (p.len() == 4).then(|| [p[0], p[1], p[2], p[3]])
}

/// "3px" / "40%" / "2mm" / "150x150" → leading number.
fn number(v: &str) -> Option<f64> {
    let n: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    n.parse().ok()
}

fn index_of<T: PartialEq>(list: &[T], v: Option<T>) -> Option<usize> {
    list.iter().position(|x| Some(x) == v.as_ref())
}

/// Preferences from NanoPix.ini, over LumoDent's defaults.
pub fn import_settings(install: &Install) -> Settings {
    let ini = &install.ini;
    let mut s = Settings::default();
    let g = |sec: &str, key: &str| ini.get(sec, key);
    if let Some(v) = g("Patient", "Automatic Chart Number") {
        s.auto_chart_number = on(v);
    }
    if let Some(v) = g("Patient", "Name Option") {
        s.last_name_first = v.starts_with("LastName");
    }
    let days = |v: Option<&str>| match v? {
        "Two days" => Some(2),
        "Three days" => Some(3),
        "Four days" => Some(4),
        _ => None,
    };
    // NanoPix spells the first one "Acquied".
    if let Some(d) = days(g("Patient", "Recently Acquied Days").or(g("Patient", "Recently Acquired Days"))) {
        s.recent_acquired_days = d;
    }
    if let Some(d) = days(g("Patient", "Recently Printed Days")) {
        s.recent_printed_days = d;
    }
    s.generator_ac = g("Acquisition", "Trigger Mode") == Some("AC");
    if let Some(i) = index_of(&KV, g("Acquisition", "Generator KV")) {
        s.kv = i;
    }
    let ma: &[&str] = if s.generator_ac { &MA_AC } else { &MA_DC };
    s.ma = index_of(ma, g("Acquisition", "Generator MA")).unwrap_or(s.ma.min(ma.len() - 1));
    if let Some(c) = g("Acquisition", "Image Calibration").and_then(|v| v.parse::<f64>().ok()) {
        s.calibration_mm = c.clamp(0.005, 1.0);
    }
    if let Some(v) = g("Viewer/General", "Full Screen Enable") {
        s.fullscreen = on(v);
    }
    if let Some(i) = index_of(&LAYOUTS, g("Viewer/General", "Image Layout Preference")) {
        s.layout = i;
    }
    if let Some(i) = index_of(&MAGNIFIER_SIZES, g("Viewer/Tools", "Magnifier Size").and_then(number).map(|n| n as u32)) {
        s.magnifier_size = i;
    }
    if let Some(i) = index_of(&MAGNIFIER_RATIOS, g("Viewer/Tools", "Magnifier Ratio").and_then(number).map(|n| n as f32)) {
        s.magnifier_ratio = i;
    }
    if let Some(c) = g("Viewer/Tools", "Capture Border Color").and_then(color) {
        s.capture_border = c;
    }
    if let Some(i) = index_of(&GRID_MM, g("Viewer/Tools", "Grid Spacing").and_then(number).map(|n| n as u32)) {
        s.grid_mm = i;
    }
    if let Some(i) = index_of(&GRID_OPACITY, g("Viewer/Tools", "Grid Line Opacity").and_then(number).map(|n| n as u32)) {
        s.grid_opacity = i;
    }
    let stroke = |st: &mut Stroke, sec: &str, c: &str, t: &str| {
        if let Some(c) = ini.get(sec, c).and_then(color) {
            st.color = c;
        }
        if let Some(t) = ini.get(sec, t).and_then(number) {
            st.thickness = (t as u32).clamp(1, 5);
        }
    };
    stroke(&mut s.grid, "Viewer/Tools", "Grid Line Color", "Grid Line Thickness");
    stroke(&mut s.draw, "Viewer/Annotation", "Draw Color", "Draw Thickness");
    stroke(&mut s.line, "Viewer/Annotation", "Line Color", "Line Thickness");
    stroke(&mut s.rectangle, "Viewer/Annotation", "Rectangle Color", "Rectangle Thickness");
    stroke(&mut s.ellipse, "Viewer/Annotation", "Ellipse Color", "Ellipse Thickness");
    stroke(&mut s.length, "Viewer/Measurement", "Length Color", "Length Line Thickness");
    stroke(&mut s.angle, "Viewer/Measurement", "Angle Color", "Angle Line Thickness");
    stroke(&mut s.border, "Report/General", "Border Line Color", "Border Line Thickness");
    if let Some(v) = g("Viewer/Measurement", "Length Unit Show") {
        s.length_unit = on(v);
    }
    if let Some(v) = g("Viewer/Measurement", "Angle Unit Show") {
        s.angle_unit = on(v);
    }
    if let Some(c) = g("Viewer/Memo", "Outer Color").and_then(color) {
        s.memo_outer = c;
    }
    if let Some(c) = g("Viewer/Memo", "Text Color").and_then(color) {
        s.memo_text = c;
    }
    for i in 0..7 {
        if let Some(p) = g("Viewer/Memo", &format!("Text Preset{}", i + 1)) {
            s.memo_presets[i] = p.to_string();
        }
    }
    if let Some(i) = index_of(&FONT_SIZES, g("Report/TextBox", "Font Size").and_then(|v| v.parse::<f32>().ok())) {
        s.font_size = i;
    }
    s
}

pub fn import_clinic(install: &Install) -> Clinic {
    let ini = &install.ini;
    let g = |k: &str| ini.get("Environment", k).unwrap_or_default().to_string();
    Clinic { name: g("Clinic Name"), address: g("Clinic Address"), phone: g("Clinic Phone") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "[Environment]\r\nClinic Name=Klinik Sehat\r\nClinic Phone=021\r\n\r\n[Patient]\r\nAutomatic Chart Number=ON\r\nName Option=LastName FirstName\r\nRecently Acquied Days=Four days\r\n\r\n[Acquisition]\r\nTrigger Mode=DC\r\nGenerator KV=65\r\nGenerator MA=1.0\r\nImage Calibration=0.030000\r\nPatient Data Directory=PLACEHOLDER\r\n\r\n[Viewer/General]\r\nFull Screen Enable=ON\r\nImage Layout Preference=2x2\r\n\r\n[Viewer/Tools]\r\nMagnifier Size=200x200\r\nMagnifier Ratio=2.5\r\nGrid Spacing=5mm\r\nGrid Line Opacity=40%\r\n\r\n[Viewer/Annotation]\r\nLine Color=255,255,0,255\r\nLine Thickness=3px\r\n\r\n[Viewer/Memo]\r\nText Preset1=Caries\r\nText Preset2=Bone Loss\r\n\r\n[Report/TextBox]\r\nFont Size=10.5\r\n";

    fn install(dir: &Path) -> PathBuf {
        let data = dir.join("PatientData");
        std::fs::create_dir_all(data.join("database")).unwrap();
        let ini = dir.join("NanoPix.ini");
        std::fs::write(&ini, INI.replace("PLACEHOLDER", data.to_str().unwrap())).unwrap();
        ini
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lumodent-nanopix-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn detects_only_with_a_database() {
        let dir = temp("detect");
        let ini = install(&dir);
        assert!(detect(&ini).is_none(), "no NanoPix.db3 yet");
        std::fs::write(dir.join("PatientData/database/NanoPix.db3"), b"").unwrap();
        let found = detect(&ini).unwrap();
        assert_eq!(found.data, dir.join("PatientData"));
        assert_eq!(found.user_db(), dir.join("PatientData/database/User.db3"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_uses_nanopix_databases_and_finds_its_images() {
        use crate::workspace::Workspace;
        let dir = temp("workspace");
        let ini = install(&dir);
        let db = dir.join("PatientData/database/NanoPix.db3");
        // NanoPix's own database, with a patient, a study and its admin.
        {
            let store = crate::patients::PatientStore::open(&db).unwrap();
            store.prepare("INSERT INTO Patient (id,last_name,first_name) VALUES ('P7','Doe','Jane')").unwrap().finish().unwrap();
            store.prepare("INSERT INTO Study (patient_id,modality,image_name,created_datetime) VALUES ('P7','IO','IMG_0001.png','2026-11-19 14:03:00')").unwrap().finish().unwrap();
            let users = crate::users::UserStore::open_shared(&dir.join("PatientData/database/User.db3")).unwrap();
            assert!(users.users().unwrap().is_empty(), "a shared user database is not seeded");
        }
        let img = dir.join("PatientData/P7/IMG_0001.png");
        std::fs::create_dir_all(img.parent().unwrap()).unwrap();
        std::fs::write(&img, b"png").unwrap();
        let found = detect(&ini).unwrap();
        let ws = Workspace::open_nanopix(dir.join("lumodent"), &found).unwrap();
        assert_eq!(ws.legacy.as_deref(), Some(found.data.as_path()));
        let studies = ws.store.studies("P7", "", crate::workspace::ALL_DATES.0, crate::workspace::ALL_DATES.1).unwrap();
        assert_eq!(studies.len(), 1);
        assert_eq!(ws.image_path(&studies[0]), img);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_preferences_and_clinic() {
        let dir = temp("import");
        let ini = install(&dir);
        std::fs::write(dir.join("PatientData/database/NanoPix.db3"), b"").unwrap();
        let found = detect(&ini).unwrap();
        let s = import_settings(&found);
        assert!(s.auto_chart_number && s.last_name_first && s.fullscreen);
        assert_eq!(s.recent_acquired_days, 4);
        assert_eq!((KV[s.kv], MA_DC[s.ma]), ("65", "1.0"));
        assert_eq!(s.calibration_mm, 0.03);
        assert_eq!(LAYOUTS[s.layout], "2x2");
        assert_eq!((MAGNIFIER_SIZES[s.magnifier_size], MAGNIFIER_RATIOS[s.magnifier_ratio]), (200, 2.5));
        assert_eq!((GRID_MM[s.grid_mm], GRID_OPACITY[s.grid_opacity]), (5, 40));
        assert_eq!(s.line, Stroke { color: [255, 255, 0, 255], thickness: 3 });
        assert_eq!(s.memo_presets[1], "Bone Loss");
        assert_eq!(FONT_SIZES[s.font_size], 10.5);
        let c = import_clinic(&found);
        assert_eq!((c.name.as_str(), c.phone.as_str(), c.address.as_str()), ("Klinik Sehat", "021", ""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
