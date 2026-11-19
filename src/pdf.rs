//! Single-page report PDF. Uses the built-in Helvetica fonts (WinAnsi text)
//! and Flate-compressed RGB images, so no font files are embedded. The page
//! uses the same blocks as the on-screen preview (`reports::layout`).

use crate::image::Image;
use crate::reports::{fit, layout, page_text, Clinic, Draft, MmRect};
use miniz_oxide::deflate::compress_to_vec_zlib;
use std::fmt::Write as _;

const PT_PER_MM: f32 = 72.0 / 25.4;
pub const TITLE_MM: f32 = 6.0;
pub const BODY_MM: f32 = 3.4;
pub const SMALL_MM: f32 = 2.8;

#[derive(Clone, Copy)]
enum Font {
    Regular,
    Bold,
}

// Advance widths for characters 32..=126 in 1/1000 em, from the standard
// Helvetica and Helvetica-Bold font metrics.
const REGULAR: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556,
    556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278,
    500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469,
    556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556,
    556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278,
    556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584,
    556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556,
    333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// WinAnsi byte for a character; characters outside Latin-1 print as '?'.
fn win_ansi(c: char) -> u8 {
    match c as u32 {
        0x20..=0x7e | 0xa0..=0xff => c as u32 as u8,
        _ => b'?',
    }
}

fn width_mm(text: &str, font: Font, size_mm: f32) -> f32 {
    let table = match font {
        Font::Regular => &REGULAR,
        Font::Bold => &BOLD,
    };
    let units: u32 = text
        .chars()
        .map(|c| match win_ansi(c) {
            b @ 0x20..=0x7e => table[(b - 0x20) as usize] as u32,
            _ => 556,
        })
        .sum();
    units as f32 / 1000.0 * size_mm
}

/// Greedy word wrap to the given width; long words are split by character.
fn wrap(text: &str, font: Font, size_mm: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if width_mm(&candidate, font, size_mm) <= width || line.is_empty() && word.is_empty() {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for c in word.chars() {
                if !line.is_empty() && width_mm(&format!("{line}{c}"), font, size_mm) > width {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(c);
            }
        }
        lines.push(line);
    }
    lines
}

/// Cuts text to fit a width, ending with "...".
fn clip(text: &str, font: Font, size_mm: f32, width: f32) -> String {
    if width_mm(text, font, size_mm) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        if width_mm(&format!("{out}{c}..."), font, size_mm) > width {
            break;
        }
        out.push(c);
    }
    out + "..."
}

struct Page {
    ops: Vec<u8>,
    height_mm: f32,
}

impl Page {
    fn op(&mut self, s: &str) {
        self.ops.extend_from_slice(s.as_bytes());
        self.ops.push(b'\n');
    }

    fn y(&self, mm: f32) -> f32 {
        (self.height_mm - mm) * PT_PER_MM
    }

    /// Draws one line with its top edge at `top_mm`.
    fn text(&mut self, text: &str, font: Font, size_mm: f32, x_mm: f32, top_mm: f32, grey: f32) {
        let name = match font {
            Font::Regular => "F1",
            Font::Bold => "F2",
        };
        let baseline = top_mm + size_mm * 0.8;
        let mut s = format!(
            "BT {grey:.2} g /{name} {:.2} Tf {:.2} {:.2} Td (",
            size_mm * PT_PER_MM,
            x_mm * PT_PER_MM,
            self.y(baseline)
        );
        for c in text.chars() {
            match win_ansi(c) {
                b'(' => s.push_str("\\("),
                b')' => s.push_str("\\)"),
                b'\\' => s.push_str("\\\\"),
                b => {
                    self.ops.extend_from_slice(s.as_bytes());
                    s.clear();
                    self.ops.push(b);
                }
            }
        }
        s.push_str(") Tj ET");
        self.op(&s);
    }

    fn rule(&mut self, left: f32, right: f32, y: f32) {
        let line = format!(
            "0.6 G 0.6 w {:.2} {:.2} m {:.2} {:.2} l S",
            left * PT_PER_MM,
            self.y(y),
            right * PT_PER_MM,
            self.y(y)
        );
        self.op(&line);
    }

    fn image(&mut self, index: usize, r: MmRect) {
        let (w, h) = ((r.2 - r.0) * PT_PER_MM, (r.3 - r.1) * PT_PER_MM);
        let (x, y) = (r.0 * PT_PER_MM, self.y(r.3));
        self.op(&format!("q {w:.2} 0 0 {h:.2} {x:.2} {y:.2} cm /Im{index} Do Q"));
        self.op(&format!("0.75 G 0.4 w {x:.2} {y:.2} {w:.2} {h:.2} re S"));
    }
}

fn rgb(image: &Image) -> Vec<u8> {
    image.pixels.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect()
}

/// Builds the PDF bytes for a report. `now` is used when the draft is unsaved.
pub fn report_pdf(draft: &Draft, clinic: &Clinic, now: &str) -> Vec<u8> {
    let (w_mm, h_mm) = draft.doc.page_mm();
    let blocks = layout((w_mm, h_mm), draft.doc.header, draft.doc.footer, draft.images.len(), !draft.doc.note.trim().is_empty());
    let text = page_text(draft, clinic, now);
    let mut page = Page { ops: Vec::new(), height_mm: h_mm };

    if let Some(h) = blocks.header {
        let width = h.2 - h.0;
        page.text(&clip(&text.clinic_name, Font::Bold, TITLE_MM, width), Font::Bold, TITLE_MM, h.0, h.1, 0.1);
        page.text(&clip(&text.clinic_contact, Font::Regular, BODY_MM, width), Font::Regular, BODY_MM, h.0, h.1 + 10.0, 0.25);
        page.rule(h.0, h.2, h.3);
    }
    let p = blocks.patient;
    let date_w = width_mm(&text.date, Font::Regular, BODY_MM);
    let name = clip(&text.patient, Font::Bold, BODY_MM, p.2 - p.0 - date_w - 4.0);
    page.text(&name, Font::Bold, BODY_MM, p.0, p.1, 0.1);
    page.text(&text.date, Font::Regular, BODY_MM, p.2 - date_w, p.1, 0.25);
    page.text(&clip(&text.patient_details, Font::Regular, BODY_MM, p.2 - p.0), Font::Regular, BODY_MM, p.0, p.1 + 6.5, 0.25);
    page.rule(p.0, p.2, p.3 + 1.0);

    let mut images = Vec::new();
    for (i, (cell, image)) in blocks.images.iter().zip(&draft.images).enumerate() {
        page.image(i, fit(*cell, image));
        images.push(image);
    }
    if let Some(n) = blocks.note {
        page.text("Note", Font::Bold, BODY_MM, n.0, n.1, 0.1);
        let line_h = BODY_MM * 1.3;
        let mut top = n.1 + 6.5;
        for line in wrap(&text.note, Font::Regular, BODY_MM, n.2 - n.0) {
            if top + BODY_MM > n.3 {
                break;
            }
            page.text(&line, Font::Regular, BODY_MM, n.0, top, 0.15);
            top += line_h;
        }
    }
    if let Some(f) = blocks.footer {
        page.rule(f.0, f.2, f.1);
        page.text(&text.footer_left, Font::Regular, SMALL_MM, f.0, f.1 + 2.5, 0.45);
        let right_w = width_mm(&text.footer_right, Font::Regular, SMALL_MM);
        page.text(&text.footer_right, Font::Regular, SMALL_MM, f.2 - right_w, f.1 + 2.5, 0.45);
    }

    // Objects: 1 catalog, 2 pages, 3 page, 4-5 fonts, 6 content, 7.. images.
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let (w_pt, h_pt) = (w_mm * PT_PER_MM, h_mm * PT_PER_MM);
    let xobjects: String = (0..images.len()).map(|i| format!("/Im{i} {} 0 R ", 7 + i)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.push(
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w_pt:.2} {h_pt:.2}] /Resources << /Font << /F1 4 0 R /F2 5 0 R >> /XObject << {xobjects}>> >> /Contents 6 0 R >>"
        )
        .into_bytes(),
    );
    for base in ["Helvetica", "Helvetica-Bold"] {
        objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>").into_bytes());
    }
    let stream = |dict: String, data: Vec<u8>| {
        let mut out = format!("<< {dict} /Filter /FlateDecode /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    };
    objects.push(stream(String::new(), compress_to_vec_zlib(&page.ops, 6)));
    for image in images {
        let dict = format!(
            "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8",
            image.width, image.height
        );
        objects.push(stream(dict, compress_to_vec_zlib(&rgb(image), 6)));
    }

    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    let _ = write!(
        table,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    out.extend(table.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patients::{Patient, PatientDetails};
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    fn sample() -> (Draft, Clinic) {
        let patient = Patient {
            pk: 1,
            id: "P-01".into(),
            details: PatientDetails { last_name: "Wijaya".into(), first_name: "Sari (Bu)".into(), ..Default::default() },
        };
        let mut draft = Draft::for_patient(&patient);
        draft.images = vec![Image { width: 2, height: 2, pixels: vec![0xff0000, 0x00ff00, 0x0000ff, 0xffffff] }; 3];
        draft.doc.note = "Caries on 36.\nRecall in six months for a follow-up radiograph of the lower left molars.".into();
        let clinic = Clinic { name: "Klinik Gigi Sehat".into(), address: "Jl. Merdeka 1".into(), phone: "022".into() };
        (draft, clinic)
    }

    #[test]
    fn writes_a_valid_single_page_pdf() {
        let (draft, clinic) = sample();
        let pdf = report_pdf(&draft, &clinic, "2026-11-19 10:00:00");
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        // Offsets are byte positions, so search the raw bytes (lossy text shifts them).
        let xref = pdf.windows(6).rposition(|w| w == b"\nxref\n").unwrap() + 1;
        let tail = std::str::from_utf8(&pdf[xref..]).unwrap();
        let startxref: usize = tail[tail.find("startxref\n").unwrap() + 10..].lines().next().unwrap().parse().unwrap();
        assert_eq!(startxref, xref);
        let text = String::from_utf8_lossy(&pdf).into_owned();
        for (n, line) in tail.lines().skip(3).take_while(|l| l.ends_with(" n ")).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(pdf[offset..].starts_with(format!("{} 0 obj", n + 1).as_bytes()));
        }
        assert_eq!(text.matches("/Subtype /Image").count(), 3);
        assert!(text.contains("/MediaBox [0 0 595.28 841.89]"));

        let start = pdf.windows(7).position(|w| w == b"stream\n").unwrap() + 7;
        let end = start + pdf[start..].windows(10).position(|w| w == b"\nendstream").unwrap();
        let content = String::from_utf8_lossy(&decompress_to_vec_zlib(&pdf[start..end]).unwrap()).into_owned();
        assert!(content.contains("(Patient: Wijaya Sari \\(Bu\\)) Tj"));
        assert!(content.contains("(Klinik Gigi Sehat) Tj"));
        assert!(content.contains("(Report date: 2026-11-19 10:00) Tj"));
        assert!(content.contains("/Im2 Do"));
        assert!(content.contains("(Generated by LumoDent) Tj"));
    }

    #[test]
    fn landscape_and_hidden_blocks() {
        let (mut draft, clinic) = sample();
        draft.doc.landscape = true;
        draft.doc.header = false;
        draft.doc.footer = false;
        let text = String::from_utf8_lossy(&report_pdf(&draft, &clinic, "now")).into_owned();
        assert!(text.contains("/MediaBox [0 0 841.89 595.28]"));
    }

    #[test]
    fn text_metrics_wrap_and_clip() {
        assert!(width_mm("iiii", Font::Regular, 10.0) < width_mm("WWWW", Font::Regular, 10.0));
        assert!(width_mm("abc", Font::Bold, 10.0) > width_mm("abc", Font::Regular, 10.0));
        let lines = wrap("one two three four five six", Font::Regular, 5.0, 30.0);
        assert!(lines.len() > 1 && lines.iter().all(|l| width_mm(l, Font::Regular, 5.0) <= 30.0));
        assert_eq!(wrap("a\n\nb", Font::Regular, 5.0, 100.0), ["a", "", "b"]);
        let cut = clip("A very long clinic name that cannot fit", Font::Bold, 6.0, 40.0);
        assert!(cut.ends_with("...") && width_mm(&cut, Font::Bold, 6.0) <= 40.0);
        assert_eq!(win_ansi('é'), 0xe9);
        assert_eq!(win_ansi('漢'), b'?');
    }
}
