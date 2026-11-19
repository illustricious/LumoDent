//! DICOM Part 10 files for single-frame radiographs, the job of the reference
//! application's `DicomProcessor`. The reader handles the transfer syntaxes
//! intraoral sensors and PACS commonly produce; the writer emits Explicit VR
//! Little Endian, 8-bit. The code is LumoDent's own (no DICOM toolkit).

use crate::image::{Image, MAX_PIXELS};
use crate::patients::Patient;
use crate::studies::Study;
use std::collections::HashMap;

const IMPLICIT_LE: &str = "1.2.840.10008.1.2";
const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
const DEFLATED_LE: &str = "1.2.840.10008.1.2.1.99";
const JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";
const JPEG_EXTENDED: &str = "1.2.840.10008.1.2.4.51";
/// Digital Intra-Oral X-Ray Image Storage - For Presentation.
pub const INTRAORAL_CLASS: &str = "1.2.840.10008.5.1.4.1.1.1.3";
/// Secondary Capture Image Storage, for colour images.
pub const SECONDARY_CAPTURE_CLASS: &str = "1.2.840.10008.5.1.4.1.1.7";
const IMPLEMENTATION_UID: &str = "2.25.118515429349616458734120539856927735093";
const IMPLEMENTATION_VERSION: &str = "LUMODENT_03";

/// The identifying attributes LumoDent reads from and writes to a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    pub patient_id: String,
    pub last_name: String,
    pub first_name: String,
    /// YYYY-MM-DD, or empty.
    pub birthdate: String,
    pub sex: String,
    /// YYYY-MM-DD HH:MM:SS from Study/Content/Acquisition date and time, or empty.
    pub datetime: String,
    pub modality: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub accession_number: String,
    pub body_part: String,
    pub description: String,
    /// Pixel Spacing (0028,0030) or Imager Pixel Spacing (0018,1164), as written.
    pub pixel_spacing: String,
}

impl Tags {
    /// Millimetres per pixel (mean of row and column spacing), when the file says.
    pub fn spacing_mm(&self) -> Option<f64> {
        let v: Vec<f64> = self.pixel_spacing.split('\\').filter_map(|s| s.trim().parse().ok()).filter(|v: &f64| *v > 0.0).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    }
}

#[derive(Debug)]
pub struct Dicom {
    pub tags: Tags,
    pub image: Image,
}

/// True when the bytes carry the Part 10 preamble and "DICM" marker.
pub fn is_dicom(data: &[u8]) -> bool {
    data.len() >= 132 && &data[128..132] == b"DICM"
}

const fn tag(group: u16, element: u16) -> u32 {
    (group as u32) << 16 | element as u32
}

const PIXEL_DATA: u32 = tag(0x7fe0, 0x0010);
const ITEM: u32 = tag(0xfffe, 0xe000);
const ITEM_END: u32 = tag(0xfffe, 0xe00d);
const SEQUENCE_END: u32 = tag(0xfffe, 0xe0dd);
const UNDEFINED: u32 = u32::MAX;

enum Pixels<'a> {
    Native(&'a [u8]),
    Fragments(Vec<&'a [u8]>),
}

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
    explicit: bool,
}

fn damaged() -> String {
    "The DICOM file is damaged or truncated.".into()
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.d.len()).ok_or_else(damaged)?;
        let out = &self.d[self.pos..end];
        self.pos = end;
        Ok(out)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn at_end(&self) -> bool {
        self.pos >= self.d.len()
    }
    fn peek_group(&self) -> Option<u16> {
        self.d.get(self.pos..self.pos + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// Reads an element header: (tag, value length).
    fn header(&mut self) -> Result<(u32, u32), String> {
        let t = tag(self.u16()?, self.u16()?);
        if t >> 16 == 0xfffe || !self.explicit {
            return Ok((t, self.u32()?));
        }
        let vr = self.bytes(2)?;
        let long = matches!(vr, b"OB" | b"OW" | b"OF" | b"SQ" | b"UT" | b"UN" | b"OD" | b"OL" | b"OV" | b"UC" | b"UR" | b"SV" | b"UV");
        if long {
            self.bytes(2)?;
            Ok((t, self.u32()?))
        } else {
            Ok((t, self.u16()? as u32))
        }
    }

    /// Skips the items of an undefined-length sequence, up to its delimiter.
    fn skip_sequence(&mut self) -> Result<(), String> {
        loop {
            let t = tag(self.u16()?, self.u16()?);
            let len = self.u32()?;
            match t {
                SEQUENCE_END => return Ok(()),
                ITEM if len == UNDEFINED => self.skip_item()?,
                ITEM => {
                    self.bytes(len as usize)?;
                }
                _ => return Err(damaged()),
            }
        }
    }

    /// Skips the elements of an undefined-length item, up to its delimiter.
    fn skip_item(&mut self) -> Result<(), String> {
        loop {
            let (t, len) = self.header()?;
            if t == ITEM_END {
                return Ok(());
            }
            self.skip_value(len)?;
        }
    }

    fn skip_value(&mut self, len: u32) -> Result<(), String> {
        if len == UNDEFINED {
            self.skip_sequence()
        } else {
            self.bytes(len as usize).map(|_| ())
        }
    }

    fn fragments(&mut self) -> Result<Vec<&'a [u8]>, String> {
        let mut out = Vec::new();
        loop {
            let t = tag(self.u16()?, self.u16()?);
            let len = self.u32()?;
            match t {
                SEQUENCE_END => return Ok(out),
                ITEM if len != UNDEFINED => out.push(self.bytes(len as usize)?),
                _ => return Err(damaged()),
            }
        }
    }
}

struct Elements<'a> {
    values: HashMap<u32, &'a [u8]>,
    pixels: Option<Pixels<'a>>,
}

impl Elements<'_> {
    fn text(&self, t: u32) -> String {
        let raw = self.values.get(&t).copied().unwrap_or_default();
        String::from_utf8_lossy(raw).trim_end_matches([' ', '\0']).trim_start().to_string()
    }
    fn first(&self, t: u32) -> Option<f64> {
        self.text(t).split('\\').next()?.trim().parse().ok()
    }
    fn us(&self, t: u32) -> Option<u16> {
        self.values.get(&t).filter(|v| v.len() >= 2).map(|v| u16::from_le_bytes([v[0], v[1]]))
    }
}

/// Reads the top-level elements of a dataset; sequences are skipped.
fn read_dataset<'a>(r: &mut Reader<'a>) -> Result<Elements<'a>, String> {
    let mut values = HashMap::new();
    let mut pixels = None;
    while !r.at_end() {
        // Trailing padding (FFFC,FFFC) or odd garbage after the pixels ends the walk.
        if r.d.len() - r.pos < 8 {
            break;
        }
        let (t, len) = r.header()?;
        if t == PIXEL_DATA {
            pixels = Some(if len == UNDEFINED { Pixels::Fragments(r.fragments()?) } else { Pixels::Native(r.bytes(len as usize)?) });
        } else if len == UNDEFINED {
            r.skip_sequence()?;
        } else {
            let v = r.bytes(len as usize)?;
            values.insert(t, v);
        }
    }
    Ok(Elements { values, pixels })
}

fn date(da: &str) -> String {
    let d: String = da.chars().filter(char::is_ascii_digit).collect();
    if d.len() >= 8 {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8])
    } else {
        String::new()
    }
}

fn time(tm: &str) -> String {
    let t: String = tm.chars().take_while(|c| *c != '.').filter(char::is_ascii_digit).collect();
    let t = format!("{t:0<6}");
    format!("{}:{}:{}", &t[..2], &t[2..4], &t[4..6])
}

/// Parses a DICOM file into its tags and an 8-bit display image.
pub fn parse(data: &[u8]) -> Result<Dicom, String> {
    let owned;
    let (meta, body, syntax) = if is_dicom(data) {
        let mut r = Reader { d: data, pos: 132, explicit: true };
        let mut meta = HashMap::new();
        while r.peek_group() == Some(0x0002) {
            let (t, len) = r.header()?;
            meta.insert(t, r.bytes(len as usize)?);
        }
        let syntax = Elements { values: meta, pixels: None }.text(tag(0x0002, 0x0010));
        (r.pos, data, syntax)
    } else if data.len() > 8 && data[..2] == [0x08, 0x00] {
        // A bare dataset without the Part 10 header: assume Implicit VR Little Endian.
        (0, data, IMPLICIT_LE.to_string())
    } else {
        return Err("Not a DICOM file.".into());
    };
    let (body, start) = if syntax == DEFLATED_LE {
        owned = miniz_oxide::inflate::decompress_to_vec_with_limit(&body[meta..], 1 << 30).map_err(|_| damaged())?;
        (&owned[..], 0)
    } else {
        (body, meta)
    };
    let explicit = match syntax.as_str() {
        IMPLICIT_LE => false,
        EXPLICIT_LE | DEFLATED_LE | JPEG_BASELINE | JPEG_EXTENDED => true,
        "1.2.840.10008.1.2.2" => return Err("Big-endian DICOM files are not supported.".into()),
        other => return Err(format!("This DICOM compression is not supported yet (transfer syntax {other}).")),
    };
    let mut r = Reader { d: body, pos: start, explicit };
    let e = read_dataset(&mut r)?;

    let name = e.text(tag(0x0010, 0x0010));
    let mut parts = name.split('^');
    let (last_name, first_name) = (parts.next().unwrap_or("").trim().to_string(), parts.next().unwrap_or("").trim().to_string());
    let datetime = [(0x0020, 0x0030), (0x0023, 0x0033), (0x0022, 0x0032)]
        .iter()
        .map(|&(d, t)| (date(&e.text(tag(0x0008, d))), e.text(tag(0x0008, t))))
        .find(|(d, _)| !d.is_empty())
        .map(|(d, t)| format!("{d} {}", time(&t)))
        .unwrap_or_default();
    let tags = Tags {
        patient_id: e.text(tag(0x0010, 0x0020)),
        last_name,
        first_name,
        birthdate: date(&e.text(tag(0x0010, 0x0030))),
        sex: e.text(tag(0x0010, 0x0040)),
        datetime,
        modality: e.text(tag(0x0008, 0x0060)),
        sop_class_uid: e.text(tag(0x0008, 0x0016)),
        sop_instance_uid: e.text(tag(0x0008, 0x0018)),
        accession_number: e.text(tag(0x0008, 0x0050)),
        body_part: e.text(tag(0x0018, 0x0015)),
        description: e.text(tag(0x0008, 0x103e)),
        pixel_spacing: Some(e.text(tag(0x0028, 0x0030))).filter(|s| !s.is_empty()).unwrap_or_else(|| e.text(tag(0x0018, 0x1164))),
    };
    let image = match &e.pixels {
        None => return Err("The DICOM file has no image.".into()),
        Some(Pixels::Fragments(frags)) => {
            // The first item is the basic offset table; the frame follows.
            let frame: Vec<u8> = frags.iter().skip(1).flat_map(|f| f.iter().copied()).collect();
            let mut image = crate::image::decode(&frame).map_err(|_| "Cannot decode the JPEG image inside the DICOM file.".to_string())?;
            if e.text(tag(0x0028, 0x0004)) == "MONOCHROME1" {
                image.invert();
            }
            image
        }
        Some(Pixels::Native(raw)) => native_image(&e, raw)?,
    };
    Ok(Dicom { tags, image })
}

fn native_image(e: &Elements, raw: &[u8]) -> Result<Image, String> {
    let rows = e.us(tag(0x0028, 0x0010)).unwrap_or(0) as usize;
    let cols = e.us(tag(0x0028, 0x0011)).unwrap_or(0) as usize;
    let samples = e.us(tag(0x0028, 0x0002)).unwrap_or(1) as usize;
    let allocated = e.us(tag(0x0028, 0x0100)).unwrap_or(8) as usize;
    let stored = e.us(tag(0x0028, 0x0101)).unwrap_or(allocated as u16) as usize;
    let signed = e.us(tag(0x0028, 0x0103)) == Some(1);
    let photometric = e.text(tag(0x0028, 0x0004));
    let n = rows.checked_mul(cols).filter(|&n| n > 0 && n <= MAX_PIXELS).ok_or("The DICOM image is empty or too large.")?;
    let bytes = allocated / 8;
    if !(allocated == 8 || allocated == 16) || stored == 0 || stored > allocated || raw.len() < n * samples * bytes {
        return Err(format!("Unsupported DICOM pixel layout ({allocated}-bit, {samples} samples)."));
    }
    if samples == 3 {
        if allocated != 8 || photometric != "RGB" {
            return Err(format!("Unsupported DICOM colour image ({photometric})."));
        }
        let planar = e.us(tag(0x0028, 0x0006)) == Some(1);
        let pixels = (0..n)
            .map(|i| {
                let c = |k: usize| if planar { raw[k * n + i] } else { raw[i * 3 + k] } as u32;
                c(0) << 16 | c(1) << 8 | c(2)
            })
            .collect();
        return Ok(Image { width: cols, height: rows, pixels });
    }
    if samples != 1 {
        return Err(format!("Unsupported DICOM image ({samples} samples per pixel)."));
    }
    let slope = e.first(tag(0x0028, 0x1053)).filter(|s| *s != 0.0).unwrap_or(1.0);
    let intercept = e.first(tag(0x0028, 0x1052)).unwrap_or(0.0);
    let mask = (1u32 << stored) - 1;
    let values: Vec<f64> = (0..n)
        .map(|i| {
            let v = if bytes == 1 { raw[i] as u32 } else { u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]) as u32 } & mask;
            let v = if signed && v >> (stored - 1) & 1 == 1 { v as i64 - (1i64 << stored) } else { v as i64 };
            v as f64 * slope + intercept
        })
        .collect();
    let window = e.first(tag(0x0028, 0x1050)).zip(e.first(tag(0x0028, 0x1051))).filter(|(_, w)| *w >= 1.0);
    let (lo, hi) = match window {
        Some((c, w)) => (c - 0.5 - (w - 1.0) / 2.0, c - 0.5 + (w - 1.0) / 2.0),
        None => values.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v))),
    };
    let span = (hi - lo).max(1e-9);
    let invert = photometric == "MONOCHROME1";
    let pixels = values
        .iter()
        .map(|&v| {
            let g = ((v - lo) / span * 255.0).round().clamp(0.0, 255.0) as u32;
            let g = if invert { 255 - g } else { g };
            g * 0x010101
        })
        .collect();
    Ok(Image { width: cols, height: rows, pixels })
}

/// A new globally unique identifier under the UUID-derived 2.25 root.
pub fn new_uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
    let mixed = nanos ^ (std::process::id() as u128) << 96 ^ (COUNTER.fetch_add(1, Ordering::Relaxed) as u128) << 64;
    format!("2.25.{}", mixed.max(1))
}

fn valid_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= 64 && uid.chars().all(|c| c.is_ascii_digit() || c == '.')
}

struct Writer(Vec<u8>);

impl Writer {
    fn element(&mut self, group: u16, element: u16, vr: &[u8; 2], value: &[u8]) {
        let pad = if vr == b"UI" || vr == b"OB" { 0 } else { b' ' };
        let len = value.len() + value.len() % 2;
        self.0.extend_from_slice(&group.to_le_bytes());
        self.0.extend_from_slice(&element.to_le_bytes());
        self.0.extend_from_slice(vr);
        if matches!(vr, b"OB" | b"OW" | b"SQ" | b"UT" | b"UN") {
            self.0.extend_from_slice(&[0, 0]);
            self.0.extend_from_slice(&(len as u32).to_le_bytes());
        } else {
            self.0.extend_from_slice(&(len as u16).to_le_bytes());
        }
        self.0.extend_from_slice(value);
        if value.len() % 2 == 1 {
            self.0.push(pad);
        }
    }
    fn text(&mut self, group: u16, element: u16, vr: &[u8; 2], value: &str) {
        self.element(group, element, vr, value.as_bytes());
    }
    fn us(&mut self, group: u16, element: u16, value: u16) {
        self.element(group, element, b"US", &value.to_le_bytes());
    }
}

/// Encodes an image with the patient's and study's attributes. Greyscale images
/// become Digital Intra-Oral X-Ray objects, colour ones Secondary Capture.
pub fn write(image: &Image, patient: &Patient, study: &Study) -> Vec<u8> {
    let grey = image.pixels.iter().all(|p| (p >> 16) & 0xff == p & 0xff && (p >> 8) & 0xff == p & 0xff);
    let class = if grey { INTRAORAL_CLASS } else { SECONDARY_CAPTURE_CLASS };
    let instance = if valid_uid(&study.sop_instance_uid) { study.sop_instance_uid.clone() } else { new_uid() };
    let digits: String = study.datetime.chars().filter(char::is_ascii_digit).collect();
    let (da, tm) = if digits.len() >= 14 { (digits[..8].to_string(), digits[8..14].to_string()) } else {
        let now = chrono::Local::now();
        (now.format("%Y%m%d").to_string(), now.format("%H%M%S").to_string())
    };
    let d = &patient.details;
    let birth: String = d.birthdate.chars().filter(char::is_ascii_digit).collect();
    let sex = match d.sex.to_ascii_uppercase().chars().next() {
        Some(c @ ('M' | 'F' | 'O')) => c.to_string(),
        _ => String::new(),
    };

    let mut meta = Writer(Vec::new());
    meta.element(0x0002, 0x0001, b"OB", &[0, 1]);
    meta.text(0x0002, 0x0002, b"UI", class);
    meta.text(0x0002, 0x0003, b"UI", &instance);
    meta.text(0x0002, 0x0010, b"UI", EXPLICIT_LE);
    meta.text(0x0002, 0x0012, b"UI", IMPLEMENTATION_UID);
    meta.text(0x0002, 0x0013, b"SH", IMPLEMENTATION_VERSION);

    let mut out = Writer(vec![0; 128]);
    out.0.extend_from_slice(b"DICM");
    out.element(0x0002, 0x0000, b"UL", &(meta.0.len() as u32).to_le_bytes());
    out.0.extend_from_slice(&meta.0);

    out.text(0x0008, 0x0008, b"CS", "DERIVED\\PRIMARY");
    out.text(0x0008, 0x0016, b"UI", class);
    out.text(0x0008, 0x0018, b"UI", &instance);
    out.text(0x0008, 0x0020, b"DA", &da);
    out.text(0x0008, 0x0030, b"TM", &tm);
    out.text(0x0008, 0x0050, b"SH", &study.accession_number);
    out.text(0x0008, 0x0060, b"CS", if grey { "IO" } else { "OT" });
    if !grey {
        out.text(0x0008, 0x0064, b"CS", "WSD");
    }
    out.text(0x0008, 0x0070, b"LO", crate::APP_NAME);
    out.text(0x0008, 0x103e, b"LO", &study.description);
    out.text(0x0010, 0x0010, b"PN", &format!("{}^{}", d.last_name, d.first_name).trim_end_matches('^').to_string());
    out.text(0x0010, 0x0020, b"LO", &patient.id);
    out.text(0x0010, 0x0030, b"DA", if birth.len() == 8 { &birth } else { "" });
    out.text(0x0010, 0x0040, b"CS", &sex);
    if !study.body_part_examined.is_empty() {
        out.text(0x0018, 0x0015, b"CS", &study.body_part_examined);
    }
    out.text(0x0020, 0x000d, b"UI", &new_uid());
    out.text(0x0020, 0x000e, b"UI", &new_uid());
    out.text(0x0020, 0x0010, b"SH", "");
    out.text(0x0020, 0x0011, b"IS", "1");
    out.text(0x0020, 0x0013, b"IS", "1");
    out.us(0x0028, 0x0002, if grey { 1 } else { 3 });
    out.text(0x0028, 0x0004, b"CS", if grey { "MONOCHROME2" } else { "RGB" });
    if !grey {
        out.us(0x0028, 0x0006, 0);
    }
    out.us(0x0028, 0x0010, image.height as u16);
    out.us(0x0028, 0x0011, image.width as u16);
    out.us(0x0028, 0x0100, 8);
    out.us(0x0028, 0x0101, 8);
    out.us(0x0028, 0x0102, 7);
    out.us(0x0028, 0x0103, 0);
    let pixels: Vec<u8> = if grey {
        image.pixels.iter().map(|p| *p as u8).collect()
    } else {
        image.pixels.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect()
    };
    out.element(0x7fe0, 0x0010, b"OB", &pixels);
    out.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patients::PatientDetails;

    fn patient() -> Patient {
        Patient {
            pk: 1,
            id: "P-01".into(),
            details: PatientDetails { last_name: "Wijaya".into(), first_name: "Sari".into(), sex: "F".into(), birthdate: "1990-04-12".into(), ..Default::default() },
        }
    }

    #[test]
    fn grey_round_trip_keeps_pixels_and_tags() {
        let image = Image { width: 3, height: 2, pixels: [0, 40, 80, 120, 200, 255].iter().map(|g| g * 0x010101).collect() };
        let study = Study { sop_instance_uid: "2.25.42".into(), datetime: "2026-11-19 08:30:05".into(), accession_number: "A7".into(), ..Default::default() };
        let bytes = write(&image, &patient(), &study);
        assert!(is_dicom(&bytes));
        let d = parse(&bytes).unwrap();
        assert_eq!(d.image, image);
        assert_eq!((d.tags.patient_id.as_str(), d.tags.last_name.as_str(), d.tags.first_name.as_str()), ("P-01", "Wijaya", "Sari"));
        assert_eq!((d.tags.birthdate.as_str(), d.tags.sex.as_str(), d.tags.modality.as_str()), ("1990-04-12", "F", "IO"));
        assert_eq!((d.tags.sop_instance_uid.as_str(), d.tags.sop_class_uid.as_str()), ("2.25.42", INTRAORAL_CLASS));
        assert_eq!((d.tags.datetime.as_str(), d.tags.accession_number.as_str()), ("2026-11-19 08:30:05", "A7"));
        assert!(crate::image::decode(&bytes).is_ok());
    }

    #[test]
    fn pixel_spacing_is_the_mean_in_millimetres() {
        let mut t = Tags { pixel_spacing: "0.1\\0.12".into(), ..Default::default() };
        assert!((t.spacing_mm().unwrap() - 0.11).abs() < 1e-9);
        t.pixel_spacing = String::new();
        assert_eq!(t.spacing_mm(), None);
    }

    #[test]
    fn colour_round_trip_uses_secondary_capture() {
        let image = Image { width: 2, height: 1, pixels: vec![0xff0000, 0x00ff80] };
        let d = parse(&write(&image, &patient(), &Study::default())).unwrap();
        assert_eq!(d.image, image);
        assert_eq!(d.tags.sop_class_uid, SECONDARY_CAPTURE_CLASS);
        assert!(valid_uid(&d.tags.sop_instance_uid));
    }

    /// An Implicit VR file with 12-bit MONOCHROME1 pixels, a window and a sequence.
    #[test]
    fn implicit_16_bit_monochrome1_with_window() {
        let mut ds = Vec::new();
        let mut el = |g: u16, e: u16, v: &[u8]| {
            ds.extend_from_slice(&g.to_le_bytes());
            ds.extend_from_slice(&e.to_le_bytes());
            ds.extend_from_slice(&(v.len() as u32).to_le_bytes());
            ds.extend_from_slice(v);
        };
        el(0x0008, 0x0060, b"IO");
        el(0x0010, 0x0020, b"X9");
        // An undefined-length sequence holding one undefined-length item.
        let mut seq = Vec::new();
        for w in [0x0008u16, 0x1140, 0xffff, 0xffff, 0xfffe, 0xe000, 0xffff, 0xffff, 0x0008, 0x1150, 2, 0, 0x3131, 0xfffe, 0xe00d, 0, 0, 0xfffe, 0xe0dd, 0, 0] {
            seq.extend_from_slice(&w.to_le_bytes());
        }
        el(0x0028, 0x0002, &1u16.to_le_bytes());
        el(0x0028, 0x0004, b"MONOCHROME1 ");
        el(0x0028, 0x0010, &1u16.to_le_bytes());
        el(0x0028, 0x0011, &3u16.to_le_bytes());
        el(0x0028, 0x0100, &16u16.to_le_bytes());
        el(0x0028, 0x0101, &12u16.to_le_bytes());
        el(0x0028, 0x1050, b"2048");
        el(0x0028, 0x1051, b"4096");
        let pixels: Vec<u8> = [0u16, 2048, 0xf000 | 4095].iter().flat_map(|v| v.to_le_bytes()).collect();
        el(0x7fe0, 0x0010, &pixels);
        // Put the sequence after the group 0010 elements.
        let at = 8 + 2 + 8 + 2;
        ds.splice(at..at, seq);
        let mut file = vec![0; 128];
        file.extend_from_slice(b"DICM");
        let mut meta = Writer(Vec::new());
        meta.text(0x0002, 0x0010, b"UI", IMPLICIT_LE);
        file.extend_from_slice(&meta.0);
        file.extend_from_slice(&ds);
        let d = parse(&file).unwrap();
        assert_eq!(d.tags.patient_id, "X9");
        let grey: Vec<u32> = d.image.pixels.iter().map(|p| p & 0xff).collect();
        // MONOCHROME1: low values are white. The high bits above 12 are masked.
        assert_eq!(grey, vec![255, 127, 0]);
    }

    #[test]
    fn rejects_other_files_and_compression() {
        assert!(parse(b"not a dicom file at all").is_err());
        let mut file = vec![0; 128];
        file.extend_from_slice(b"DICM");
        let mut meta = Writer(Vec::new());
        meta.text(0x0002, 0x0010, b"UI", "1.2.840.10008.1.2.4.90");
        file.extend_from_slice(&meta.0);
        assert!(parse(&file).unwrap_err().contains("not supported yet"));
        assert!(parse(&file[..100]).is_err());
    }
}
