//! Report screen: saved reports, page settings, clinic header, note, images
//! sent from the viewer, a live page preview, PDF export and printing.

use super::icons::{self, Icon, Layout as IconLayout};
use super::viewer::{color_image, thumbnail};
use super::{list_row, section, t, theme, App, Pending};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Rect, Stroke, TextureHandle, TextureOptions, Ui};
use lumodent::pdf::{report_pdf, BODY_MM, SMALL_MM, TITLE_MM};
use lumodent::reports::{fit, layout, page_text, Clinic, Draft, MmRect, Paper};
use lumodent::studies::Report;

pub struct ReportState {
    pub draft: Draft,
    pub clinic: Clinic,
    pub rows: Vec<Report>,
    pub selected_image: Option<usize>,
    /// Bumped whenever the draft's images change, to rebuild textures.
    generation: u64,
    textures: (u64, Vec<TextureHandle>, Vec<TextureHandle>),
}

impl ReportState {
    pub fn new(clinic: Clinic) -> Self {
        Self { draft: Draft::default(), clinic, rows: Vec::new(), selected_image: None, generation: 1, textures: (0, Vec::new(), Vec::new()) }
    }

    pub fn changed(&mut self) {
        self.generation += 1;
        self.selected_image = None;
    }

    /// Full-size page textures and list thumbnails for the draft's images.
    fn textures(&mut self, ctx: &egui::Context) -> (Vec<egui::TextureId>, Vec<egui::TextureId>) {
        if self.textures.0 != self.generation {
            let page = self.draft.images.iter().enumerate().map(|(i, im)| {
                ctx.load_texture(format!("report-{}-{i}", self.generation), color_image(im.width, im.height, &im.pixels), TextureOptions::LINEAR)
            });
            let thumbs = self.draft.images.iter().enumerate().map(|(i, im)| {
                ctx.load_texture(format!("report-thumb-{}-{i}", self.generation), thumbnail(im, 128), TextureOptions::LINEAR)
            });
            self.textures = (self.generation, page.collect(), thumbs.collect());
        }
        (self.textures.1.iter().map(|t| t.id()).collect(), self.textures.2.iter().map(|t| t.id()).collect())
    }
}

impl App {
    pub fn enter_report(&mut self, patient: lumodent::patients::Patient) -> Result<(), String> {
        if self.report.draft.patient.as_ref().map(|p| p.pk) != Some(patient.pk) {
            self.report.draft = Draft::for_patient(&patient);
            self.report.changed();
        }
        self.load_reports()
    }

    pub fn load_reports(&mut self) -> Result<(), String> {
        let Some(patient) = self.report.draft.patient.clone() else { return Ok(()) };
        self.report.rows = self.ws.reports(&patient)?;
        Ok(())
    }

    pub fn reset_draft(&mut self) {
        if let Some(patient) = self.report.draft.patient.clone() {
            self.report.draft = Draft::for_patient(&patient);
            self.report.changed();
        }
    }

    pub fn load_report(&mut self, index: usize) -> Result<(), String> {
        let (Some(patient), Some(row)) = (self.report.draft.patient.clone(), self.report.rows.get(index).cloned()) else {
            return Ok(());
        };
        self.report.draft = self.ws.load_report(&patient, &row)?;
        self.report.changed();
        Ok(())
    }

    fn unsaved(&self) -> bool {
        self.report.draft.dirty && !self.report.draft.is_empty()
    }

    pub fn save_report(&mut self) -> Result<(), String> {
        self.ws.save_clinic(&self.report.clinic)?;
        self.ws.save_report(&mut self.report.draft)?;
        self.load_reports()?;
        self.notice = Some(t("Save report successful, you can reload the report and edit it later.").into());
        Ok(())
    }

    pub fn delete_report(&mut self, row: &Report) -> Result<(), String> {
        self.ws.delete_report(row)?;
        if self.report.draft.pk == Some(row.pk) {
            self.report.draft.pk = None;
            self.report.draft.project = None;
            self.report.draft.created.clear();
            self.report.draft.dirty = true;
        }
        self.load_reports()
    }

    fn pdf_name(&self) -> String {
        let id = self.report.draft.patient.as_ref().map(|p| lumodent::studies::folder_name(&p.id)).unwrap_or_default();
        match &self.report.draft.project {
            Some(project) => format!("{project}.pdf"),
            None => format!("{id}_report.pdf"),
        }
    }

    fn pdf_bytes(&self) -> Result<Vec<u8>, String> {
        if self.report.draft.is_empty() {
            return Err(t("Unable to print an empty report.").into());
        }
        Ok(report_pdf(&self.report.draft, &self.report.clinic, &lumodent::dates::local_timestamp()))
    }

    pub fn export_pdf(&mut self) -> Result<(), String> {
        let bytes = self.pdf_bytes()?;
        let name = self.pdf_name();
        let Some(path) = (self.hooks.save_file)(&name, "pdf") else { return Ok(()) };
        std::fs::write(&path, bytes).map_err(|e| format!("Export PDF failed: {e}"))?;
        self.notice = Some(super::i18n::pick(format!("Report exported to {}", path.display()), format!("Laporan diekspor ke {}", path.display())));
        Ok(())
    }

    /// Opens the report as a PDF in the system viewer, which prints it.
    pub fn print_report(&mut self) -> Result<(), String> {
        let bytes = self.pdf_bytes()?;
        let folder = self.ws.data.join("print");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = folder.join(self.pdf_name());
        std::fs::write(&path, bytes).map_err(|e| format!("Print report failed: {e}"))?;
        (self.hooks.open_file)(&path).map_err(|e| format!("Print report failed: {e}"))
    }

    pub(super) fn report_screen(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let (page_textures, thumbs) = self.report.textures(&ctx);
        egui::Panel::left("report-left")
            .exact_size(self.left_width)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(ui, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(ui.available_height());
                    section(ui, t("REPORT LIST"));
                    let width = ui.available_width();
                    let selected = self.report.rows.iter().position(|r| Some(r.pk) == self.report.draft.pk);
                    let (mut new, mut delete, mut open) = (false, false, None);
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        delete = icons::button(ui, Some(Icon::Delete), t("Delete Report"), vec2(width, 32.0), IconLayout::Inline, selected.is_some(), false).clicked();
                        new = icons::button(ui, Some(Icon::NewDoc), t("New Report"), vec2(width, 32.0), IconLayout::Inline, true, false).clicked();
                        ui.add_space(4.0);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                for (i, r) in self.report.rows.iter().enumerate() {
                                    let when = r.created_datetime.get(5..16).unwrap_or(&r.created_datetime);
                                    let label = format!("{}.  {when}", i + 1);
                                    if list_row(ui, &label, &[(1.0, &label)], 30.0, selected == Some(i), i % 2 == 1, None, vec2(0.0, 0.0)).clicked() {
                                        open = Some(i);
                                    }
                                }
                            });
                        });
                    });
                    if let Some(i) = open.filter(|&i| Some(i) != selected) {
                        if self.unsaved() {
                            self.ask(t("The current report has not been saved. Discard the changes?"), Pending::DiscardForReport(i));
                        } else {
                            let result = self.load_report(i);
                            self.check(result);
                        }
                    }
                    if new {
                        if self.unsaved() {
                            self.ask(t("The current report has not been saved. Discard the changes?"), Pending::DiscardForNewReport);
                        } else {
                            self.reset_draft();
                        }
                    }
                    if delete {
                        if let Some(row) = selected.and_then(|i| self.report.rows.get(i)).cloned() {
                            self.ask(t("The selected report will be deleted, are you sure to do so?"), Pending::DeleteReport(row));
                        }
                    }
                });
            });

        egui::Panel::right("report-right")
            .exact_size(280.0)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 8, bottom: 8 }))
            .show(ui, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(ui.available_height());
                    section(ui, t("CLINIC INFORMATION"));
                    let clinic = &mut self.report.clinic;
                    for (label, value, rows) in [(t("Clinic Name"), &mut clinic.name, 1), (t("Clinic Address"), &mut clinic.address, 2), (t("Clinic Phone"), &mut clinic.phone, 1)] {
                        let caption = ui.label(label);
                        let edit = if rows == 1 {
                            egui::TextEdit::singleline(value)
                        } else {
                            egui::TextEdit::multiline(value).desired_rows(rows)
                        };
                        ui.add(edit.desired_width(f32::INFINITY)).labelled_by(caption.id);
                    }
                    ui.add_space(4.0);
                    let caption = ui.label(t("Note"));
                    let note = ui.add(egui::TextEdit::multiline(&mut self.report.draft.doc.note).desired_rows(5).desired_width(f32::INFINITY));
                    if note.labelled_by(caption.id).changed() {
                        self.report.draft.dirty = true;
                    }
                    ui.add_space(4.0);
                    section(ui, t("REPORT IMAGES"));
                    let width = ui.available_width();
                    let mut remove = false;
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        remove = icons::button(ui, Some(Icon::Delete), t("Remove Image"), vec2(width, 32.0), IconLayout::Inline, self.report.selected_image.is_some(), false).clicked();
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                for (i, thumb) in thumbs.iter().enumerate() {
                                    let label = format!("Image {}", i + 1);
                                    let selected = self.report.selected_image == Some(i);
                                    if list_row(ui, &label, &[(1.0, &label)], 52.0, selected, false, Some(*thumb), vec2(52.0, 40.0)).clicked() {
                                        self.report.selected_image = Some(i);
                                    }
                                }
                            });
                        });
                    });
                    if remove {
                        if let Some(i) = self.report.selected_image.filter(|&i| i < self.report.draft.images.len()) {
                            self.report.draft.images.remove(i);
                            self.report.draft.dirty = true;
                            self.report.changed();
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 8, bottom: 8 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let doc = &mut self.report.draft.doc;
                    let before = (doc.paper, doc.landscape, doc.header, doc.footer);
                    egui::Grid::new("page-settings").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                        let caption = ui.label(t("Paper Size"));
                        let combo = egui::ComboBox::from_id_salt("paper").width(110.0).selected_text(doc.paper.name()).show_ui(ui, |ui| {
                            for paper in Paper::ALL {
                                ui.selectable_value(&mut doc.paper, paper, paper.name());
                            }
                        });
                        combo.response.labelled_by(caption.id);
                        ui.end_row();
                        let caption = ui.label(t("Orientation"));
                        let combo = egui::ComboBox::from_id_salt("orientation")
                            .width(110.0)
                            .selected_text(if doc.landscape { t("Landscape") } else { t("Portrait") })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut doc.landscape, false, t("Portrait"));
                                ui.selectable_value(&mut doc.landscape, true, t("Landscape"));
                            });
                        combo.response.labelled_by(caption.id);
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.checkbox(&mut doc.header, t("Header Show"));
                        ui.checkbox(&mut doc.footer, t("Footer Show"));
                    });
                    if (doc.paper, doc.landscape, doc.header, doc.footer) != before {
                        self.report.draft.dirty = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let size = vec2(88.0, 58.0);
                        let ready = !self.report.draft.is_empty();
                        if icons::button(ui, Some(Icon::Print), t("Print"), size, IconLayout::Stacked, ready, false).clicked() {
                            let result = self.print_report();
                            self.check(result);
                        }
                        if icons::button(ui, Some(Icon::Pdf), t("Export PDF"), size, IconLayout::Stacked, ready, false).clicked() {
                            let result = self.export_pdf();
                            self.check(result);
                        }
                        if icons::button(ui, Some(Icon::Save), t("Save Report"), size, IconLayout::Stacked, true, false).clicked() {
                            let result = self.save_report();
                            self.check(result);
                        }
                    });
                });
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                ui.painter().rect_filled(rect, 0.0, theme::edge());
                self.paint_page(ui, rect, &page_textures);
            });
    }

    /// Draws the page as the PDF will look, scaled to fit `area`.
    fn paint_page(&self, ui: &Ui, area: Rect, textures: &[egui::TextureId]) {
        let draft = &self.report.draft;
        let page = draft.doc.page_mm();
        let scale = ((area.width() - 24.0) / page.0).min((area.height() - 24.0) / page.1).max(0.1);
        let size = vec2(page.0 * scale, page.1 * scale);
        let origin = area.center() - size / 2.0;
        let painter = ui.painter_at(area);
        let to = |r: MmRect| Rect::from_min_max(origin + vec2(r.0, r.1) * scale, origin + vec2(r.2, r.3) * scale);
        painter.rect_filled(Rect::from_min_size(origin + vec2(4.0, 4.0), size), 0.0, Color32::BLACK);
        painter.rect_filled(Rect::from_min_size(origin, size), 0.0, Color32::WHITE);
        let blocks = layout(page, draft.doc.header, draft.doc.footer, draft.images.len(), !draft.doc.note.trim().is_empty());
        let text = page_text(draft, &self.report.clinic, &lumodent::dates::local_timestamp());
        let font = |mm: f32| FontId::proportional((mm * scale).max(1.0));
        let dark = Color32::from_gray(30);
        let grey = Color32::from_gray(90);
        let rule = |y: f32, left: f32, right: f32| {
            painter.line_segment([origin + vec2(left, y) * scale, origin + vec2(right, y) * scale], Stroke::new(1.0, Color32::from_gray(160)));
        };
        let line = |r: MmRect, s: &str, mm: f32, color: Color32, align: Align2| {
            let rect = to(r);
            let clip = painter.with_clip_rect(rect);
            let anchor = if align == Align2::RIGHT_TOP { rect.right_top() } else { rect.left_top() };
            clip.text(anchor, align, s, font(mm), color);
        };
        if let Some(h) = blocks.header {
            line((h.0, h.1, h.2, h.1 + 9.0), &text.clinic_name, TITLE_MM, dark, Align2::LEFT_TOP);
            line((h.0, h.1 + 10.0, h.2, h.3), &text.clinic_contact, BODY_MM, grey, Align2::LEFT_TOP);
            rule(h.3, h.0, h.2);
        }
        let p = blocks.patient;
        line((p.0, p.1, p.2, p.1 + 6.0), &text.date, BODY_MM, grey, Align2::RIGHT_TOP);
        line((p.0, p.1, p.2 - 60.0, p.1 + 6.0), &text.patient, BODY_MM, dark, Align2::LEFT_TOP);
        line((p.0, p.1 + 6.5, p.2, p.3), &text.patient_details, BODY_MM, grey, Align2::LEFT_TOP);
        rule(p.3 + 1.0, p.0, p.2);
        for ((cell, image), texture) in blocks.images.iter().zip(&draft.images).zip(textures) {
            let r = to(fit(*cell, image));
            painter.image(*texture, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(190)), egui::StrokeKind::Outside);
        }
        if let Some(n) = blocks.note {
            line((n.0, n.1, n.2, n.1 + 6.0), t("Note"), BODY_MM, dark, Align2::LEFT_TOP);
            let body = to((n.0, n.1 + 6.5, n.2, n.3));
            let galley = painter.layout(text.note.clone(), font(BODY_MM), dark, body.width());
            painter.with_clip_rect(body).galley(body.min, galley, dark);
        }
        if let Some(f) = blocks.footer {
            rule(f.1, f.0, f.2);
            line((f.0, f.1 + 2.5, f.2, f.3), &text.footer_left, SMALL_MM, grey, Align2::LEFT_TOP);
            line((f.0, f.1 + 2.5, f.2, f.3), &text.footer_right, SMALL_MM, grey, Align2::RIGHT_TOP);
        }
    }
}
