//! Report documents: page settings, clinic header, note and the images sent
//! from the viewer. Labels follow the reference Report screen (Paper Size,
//! Paper Orientation, Header Show, Footer Show, Clinic Name/Address/Phone,
//! Note). The project file format and page layout are rewrite choices.

use crate::image::Image;
use crate::patients::Patient;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paper {
    #[default]
    A4,
    A5,
    Letter,
    B5,
}

impl Paper {
    pub const ALL: [Paper; 4] = [Paper::A4, Paper::A5, Paper::Letter, Paper::B5];

    pub fn name(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::A5 => "A5",
            Paper::Letter => "Letter",
            Paper::B5 => "B5 (JIS)",
        }
    }

    /// Portrait width and height in millimetres.
    pub fn size_mm(self) -> (f32, f32) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::A5 => (148.0, 210.0),
            Paper::Letter => (215.9, 279.4),
            Paper::B5 => (182.0, 257.0),
        }
    }

    /// The matching DEVMODE dmPaperSize value.
    pub fn devmode(self) -> i16 {
        match self {
            Paper::A4 => 9,
            Paper::A5 => 11,
            Paper::Letter => 1,
            Paper::B5 => 13,
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDoc {
    pub paper: Paper,
    pub landscape: bool,
    pub header: bool,
    pub footer: bool,
    pub note: String,
    /// Image file names inside the project folder, in page order.
    pub images: Vec<String>,
}

impl Default for ReportDoc {
    fn default() -> Self {
        Self { paper: Paper::A4, landscape: false, header: true, footer: true, note: String::new(), images: Vec::new() }
    }
}

impl ReportDoc {
    pub fn page_mm(&self) -> (f32, f32) {
        let (w, h) = self.paper.size_mm();
        if self.landscape {
            (h, w)
        } else {
            (w, h)
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("lumodent-report 1\n");
        let mut put = |k: &str, v: &str| out.push_str(&format!("{k}={}\n", escape(v)));
        put("paper", self.paper.name());
        put("orientation", if self.landscape { "landscape" } else { "portrait" });
        put("header", if self.header { "1" } else { "0" });
        put("footer", if self.footer { "1" } else { "0" });
        put("note", &self.note);
        for image in &self.images {
            put("image", image);
        }
        out
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some("lumodent-report 1") {
            return Err("Not a report project file".into());
        }
        let mut doc = Self { images: Vec::new(), ..Self::default() };
        for line in lines.filter(|l| !l.is_empty()) {
            let (key, value) = line.split_once('=').ok_or("Damaged report project file")?;
            let value = unescape(value);
            match key {
                "paper" => doc.paper = Paper::parse(&value).ok_or("Unknown paper size")?,
                "orientation" => doc.landscape = value == "landscape",
                "header" => doc.header = value == "1",
                "footer" => doc.footer = value == "1",
                "note" => doc.note = value,
                // Only plain names inside the project folder are accepted.
                "image" if is_plain_name(&value) => doc.images.push(value),
                "image" => return Err("Damaged report project file".into()),
                _ => {}
            }
        }
        Ok(doc)
    }
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':']) && name != "." && name != ".."
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Clinic {
    pub name: String,
    pub address: String,
    pub phone: String,
}

impl Clinic {
    pub fn to_text(&self) -> String {
        format!(
            "clinic_name={}\nclinic_address={}\nclinic_phone={}\n",
            escape(&self.name),
            escape(&self.address),
            escape(&self.phone)
        )
    }

    pub fn from_text(text: &str) -> Self {
        let mut clinic = Self::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            match key {
                "clinic_name" => clinic.name = unescape(value),
                "clinic_address" => clinic.address = unescape(value),
                "clinic_phone" => clinic.phone = unescape(value),
                _ => {}
            }
        }
        clinic
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map(|t| Self::from_text(&t)).unwrap_or_default()
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\r', "").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            (c, false) => out.push(c),
        }
    }
    out
}

/// Rectangle in millimetres: left, top, right, bottom.
pub type MmRect = (f32, f32, f32, f32);

#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    pub header: Option<MmRect>,
    pub patient: MmRect,
    pub images: Vec<MmRect>,
    pub note: Option<MmRect>,
    pub footer: Option<MmRect>,
}

pub const MARGIN_MM: f32 = 12.0;

/// Places the report blocks top to bottom: clinic header, patient line, an
/// image grid filling the free space, the note, then the footer.
pub fn layout(page: (f32, f32), header: bool, footer: bool, images: usize, note: bool) -> PageLayout {
    let (w, h) = page;
    let (left, right) = (MARGIN_MM, w - MARGIN_MM);
    let mut top = MARGIN_MM;
    let mut bottom = h - MARGIN_MM;
    let header_rect = header.then(|| {
        let r = (left, top, right, top + 22.0);
        top += 24.0;
        r
    });
    let patient = (left, top, right, top + 14.0);
    top += 17.0;
    let footer_rect = footer.then(|| {
        let r = (left, bottom - 8.0, right, bottom);
        bottom -= 10.0;
        r
    });
    let note_rect = note.then(|| {
        let r = (left, bottom - 40.0, right, bottom);
        bottom -= 43.0;
        r
    });
    let mut cells = Vec::new();
    if images > 0 && bottom > top {
        let (area_w, area_h) = (right - left, bottom - top);
        // Pick the column count whose cells come closest to the 4:3 sensor shape.
        let columns = (1..=images)
            .min_by(|&a, &b| {
                let score = |c: usize| {
                    let rows = images.div_ceil(c);
                    let ratio = (area_w / c as f32) / (area_h / rows as f32);
                    (ratio.ln() - (4.0f32 / 3.0).ln()).abs()
                };
                score(a).total_cmp(&score(b))
            })
            .unwrap_or(1);
        let rows = images.div_ceil(columns);
        let gap = 3.0;
        let cw = (area_w - gap * (columns - 1) as f32) / columns as f32;
        let ch = (area_h - gap * (rows - 1) as f32) / rows as f32;
        for i in 0..images {
            let (c, r) = ((i % columns) as f32, (i / columns) as f32);
            let x = left + c * (cw + gap);
            let y = top + r * (ch + gap);
            cells.push((x, y, x + cw, y + ch));
        }
    }
    PageLayout { header: header_rect, patient, images: cells, note: note_rect, footer: footer_rect }
}

/// Fits an image inside a cell, keeping its aspect ratio and centring it.
pub fn fit(cell: MmRect, image: &Image) -> MmRect {
    let (cw, ch) = (cell.2 - cell.0, cell.3 - cell.1);
    let scale = (cw / image.width.max(1) as f32).min(ch / image.height.max(1) as f32);
    let (w, h) = (image.width as f32 * scale, image.height as f32 * scale);
    let (x, y) = (cell.0 + (cw - w) / 2.0, cell.1 + (ch - h) / 2.0);
    (x, y, x + w, y + h)
}

/// Writes a top-down 24-bit BMP, the format report images are stored in.
pub fn encode_bmp(image: &Image) -> Vec<u8> {
    let stride = (image.width * 3).div_ceil(4) * 4;
    let size = 54 + stride * image.height;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    for v in [size as u32, 0, 54, 40, image.width as u32, (-(image.height as i32)) as u32] {
        out.extend(v.to_le_bytes());
    }
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 24]);
    for row in image.pixels.chunks(image.width) {
        for &p in row {
            out.extend(&p.to_le_bytes()[..3]);
        }
        out.resize(out.len() + stride - image.width * 3, 0);
    }
    out
}

/// The report being edited: saved state plus the images sent from the viewer.
#[derive(Debug, Default, Clone)]
pub struct Draft {
    pub patient: Option<Patient>,
    pub pk: Option<i64>,
    pub project: Option<String>,
    pub created: String,
    pub doc: ReportDoc,
    pub images: Vec<Image>,
    pub dirty: bool,
}

impl Draft {
    pub fn for_patient(patient: &Patient) -> Self {
        Self { patient: Some(patient.clone()), ..Self::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty() && self.doc.note.trim().is_empty()
    }
}

/// Every string printed on a report page, shared by the preview and the PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageText {
    pub clinic_name: String,
    pub clinic_contact: String,
    pub patient: String,
    pub patient_details: String,
    pub date: String,
    pub note: String,
    pub footer_left: String,
    pub footer_right: String,
}

pub fn page_text(draft: &Draft, clinic: &Clinic, now: &str) -> PageText {
    let (patient, patient_details) = draft
        .patient
        .as_ref()
        .map(|p| {
            let d = &p.details;
            (
                format!("Patient: {}", d.full_name()),
                format!("Chart No.: {}     Gender: {}     Date of Birth: {}", p.id, d.sex, d.birthdate),
            )
        })
        .unwrap_or_default();
    let date = if draft.created.is_empty() { now } else { &draft.created };
    PageText {
        clinic_name: if clinic.name.is_empty() { "Clinic Name".into() } else { clinic.name.clone() },
        clinic_contact: [clinic.address.replace('\n', ", "), clinic.phone.clone()]
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("   ·   "),
        patient,
        patient_details,
        date: format!("Report date: {}", date.get(..16).unwrap_or(date)),
        note: draft.doc.note.clone(),
        footer_left: format!("Generated by {}", crate::APP_NAME),
        footer_right: "Page 1 / 1".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode_bmp;

    #[test]
    fn project_text_round_trips_and_rejects_paths() {
        let doc = ReportDoc {
            paper: Paper::B5,
            landscape: true,
            header: false,
            footer: true,
            note: "Caries on 36.\r\nRecall in 6 months \\ check 46=ok".into(),
            images: vec!["img_01.bmp".into(), "img_02.bmp".into()],
        };
        let back = ReportDoc::from_text(&doc.to_text()).unwrap();
        assert_eq!(back, ReportDoc { note: "Caries on 36.\nRecall in 6 months \\ check 46=ok".into(), ..doc });
        // In the file a backslash escapes; `\\` decodes to a real separator.
        for evil in ["../x.bmp", "..\\\\x.bmp", "C:x.bmp", ".."] {
            let text = format!("lumodent-report 1\nimage={evil}\n");
            assert!(ReportDoc::from_text(&text).is_err(), "{evil}");
        }
        assert!(ReportDoc::from_text("something else").is_err());
        assert_eq!(ReportDoc::default().page_mm(), (210.0, 297.0));
        assert_eq!(back.page_mm(), (257.0, 182.0));

        let clinic = Clinic { name: "Klinik Gigi Sehat".into(), address: "Jl. Merdeka 1\nBandung".into(), phone: "022".into() };
        assert_eq!(Clinic::from_text(&clinic.to_text()), clinic);
    }

    #[test]
    fn layout_stacks_blocks_inside_margins() {
        let page = (210.0, 297.0);
        let full = layout(page, true, true, 4, true);
        let (h, p, n, f) = (full.header.unwrap(), full.patient, full.note.unwrap(), full.footer.unwrap());
        assert!(h.3 <= p.1 && p.3 <= full.images[0].1);
        assert!(full.images.iter().all(|c| c.0 >= MARGIN_MM && c.2 <= 210.0 - MARGIN_MM && c.3 <= n.1));
        assert!(n.3 <= f.1 && f.3 <= 297.0 - MARGIN_MM);
        // Four images on portrait A4 become two columns by two rows.
        assert_eq!(full.images[1].1, full.images[0].1);
        assert!(full.images[2].1 > full.images[0].1);

        let bare = layout(page, false, false, 1, false);
        assert!(bare.header.is_none() && bare.footer.is_none() && bare.note.is_none());
        assert_eq!(bare.images[0].3, 297.0 - MARGIN_MM);
        assert!(layout(page, true, true, 0, true).images.is_empty());

        let wide_image = Image { width: 400, height: 100, pixels: vec![0; 40_000] };
        assert_eq!(fit((0.0, 0.0, 100.0, 100.0), &wide_image), (0.0, 37.5, 100.0, 62.5));
    }

    #[test]
    fn bmp_encoder_round_trips() {
        let image = Image { width: 3, height: 2, pixels: vec![0xff0000, 0x00ff00, 0x0000ff, 0x123456, 0, 0xffffff] };
        assert_eq!(decode_bmp(&encode_bmp(&image)).unwrap(), image);
    }
}
