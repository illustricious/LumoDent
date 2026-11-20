//! NanoPix's `.iosm` image files: a zstd frame (src/zstd.rs) around a record
//! with a 0x94-byte header, the processed radiograph NanoPix shows (16-bit
//! little-endian, bright = dense), the raw frame, and the DICOM fields as
//! length-prefixed strings. The header gives the image size as two floats,
//! width at 0x2C and height at 0x30 (stored landscape), and the processed
//! image's byte length at
//! 0x94, right before its pixels. Rotation is a viewer setting.

use crate::image::{Image, MAX_PIXELS};

const HEADER: usize = 0x94;

pub fn is_iosm(data: &[u8]) -> bool {
    data.starts_with(&[0x28, 0xB5, 0x2F, 0xFD])
}

pub fn decode(data: &[u8]) -> Result<Image, String> {
    let rec = crate::zstd::decompress(data, MAX_PIXELS * 4)?;
    let bad = || "Unsupported or damaged NanoPix image (.iosm)".to_string();
    let f32_at = |o: usize| rec.get(o..o + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(bad);
    let u32_at = |o: usize| rec.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(bad);
    let (width, height) = (f32_at(0x2C)? as usize, f32_at(0x30)? as usize);
    let len = u32_at(HEADER)?;
    if width == 0 || height == 0 || width * height > MAX_PIXELS || len != width * height * 2 {
        return Err(bad());
    }
    let px: Vec<u16> = rec
        .get(HEADER + 4..HEADER + 4 + len)
        .ok_or_else(bad)?
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    // NanoPix stores the processed image at full 16-bit range; show the
    // 1st-99th percentile so the radiograph fills the grey scale.
    let mut hist = vec![0usize; 65536];
    for &v in &px {
        hist[v as usize] += 1;
    }
    let pct = |p: usize| {
        let target = px.len() * p / 100;
        let mut sum = 0;
        hist.iter().position(|&c| {
            sum += c;
            sum > target
        }).unwrap_or(65535) as f32
    };
    let (lo, hi) = (pct(1), pct(99).max(pct(1) + 1.0));
    let pixels = px
        .iter()
        .map(|&v| {
            let g = ((v as f32 - lo) * 255.0 / (hi - lo)).clamp(0.0, 255.0) as u32;
            g << 16 | g << 8 | g
        })
        .collect();
    Ok(Image { width, height, pixels })
}

/// A NanoPix sample folder for tests (never committed: it holds patient data).
#[cfg(test)]
pub(crate) fn sample_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(std::path::PathBuf::from);
    [Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../PatientData")), home.map(|h| h.join("Documents/PatientData"))]
        .into_iter()
        .flatten()
        .find(|d| d.join("database/NanoPix.db3").is_file())
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_samples_when_present() {
        let Some(dir) = crate::iosm::sample_dir() else { return };
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for e in entries.flatten().filter(|e| e.path().is_dir()) {
            for f in std::fs::read_dir(e.path()).unwrap().flatten() {
                if f.path().extension().is_some_and(|x| x == "iosm") {
                    let img = super::decode(&std::fs::read(f.path()).unwrap()).unwrap();
                    assert_eq!((img.width, img.height), (1600, 1200));
                    if let Some(out) = std::env::var_os("LUMODENT_SHOT") {
                        img.save_png(&std::path::Path::new(&out).join(f.file_name()).with_extension("png")).unwrap();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod workspace_tests {
    /// A copy of the NanoPix sample folder opens through Settings' folder
    /// choice and every study's image loads.
    #[test]
    fn nanopix_folder_images_load_when_present() {
        let Some(src) = crate::iosm::sample_dir() else { return };
        let tmp = std::env::temp_dir().join(format!("lumodent-iosm-{}", std::process::id()));
        let copy = tmp.join("PatientData");
        fn cp(a: &std::path::Path, b: &std::path::Path) {
            std::fs::create_dir_all(b).unwrap();
            for e in std::fs::read_dir(a).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() { cp(&p, &b.join(e.file_name())) } else { std::fs::copy(&p, b.join(e.file_name())).unwrap(); }
            }
        }
        cp(&src, &copy);
        let ws = crate::workspace::Workspace::use_nanopix_folder(tmp.join("data"), &copy).unwrap();
        let patients = ws.store.list_created_between("", "0000-01-01", "9999-12-31").unwrap();
        let mut loaded = 0;
        for p in &patients {
            for s in ws.studies(p).unwrap() {
                let img = ws.load_image(&s).unwrap();
                assert_eq!((img.width, img.height), (1600, 1200));
                loaded += 1;
            }
        }
        assert!(loaded >= 4, "{loaded}");
        assert!(tmp.join("data/nanopix-data.txt").is_file());
        drop(ws);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
