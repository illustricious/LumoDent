//! Viewer screen: the patient's images with thumbnails, image tools, a canvas
//! with drag-to-pan and wheel zoom, and brightness/contrast.

use super::icons::{self, Icon, Layout as IconLayout};
use super::{list_row, section, t, theme, App, Pending};
use eframe::egui::{self, pos2, vec2, Align, Color32, ColorImage, Layout, Rect, TextureHandle, TextureOptions, Ui};
use lumodent::image::{levels, Image, Viewport};
use lumodent::processing;
use lumodent::patients::Patient;
use lumodent::reports::Draft;
use lumodent::studies::Study;
use std::collections::HashMap;

#[derive(Default)]
pub struct ViewerState {
    pub patient: Option<Patient>,
    pub studies: Vec<Study>,
    pub current: Option<usize>,
    pub image: Option<Image>,
    display: Vec<u32>,
    pub brightness: i32,
    pub contrast: i32,
    pub viewport: Viewport,
    pub message: String,
    texture: Option<TextureHandle>,
    texture_stale: bool,
    thumbs: HashMap<i64, Option<TextureHandle>>,
    /// Millimetres per pixel of the open image, when its file says.
    pub spacing: Option<f64>,
    /// What a drag or click on the canvas does.
    pub tool: Tool,
    /// Rulers in image pixel coordinates, cleared when the image changes.
    pub measures: Vec<[egui::Pos2; 2]>,
    /// Drawn annotations in image pixel coordinates, cleared with the rulers.
    pub marks: Vec<Mark>,
    /// The shape being dragged out (two points, or every point for free drawing).
    stroke: Vec<egui::Pos2>,
    /// A memo being typed: where it goes and its text.
    pub memo_edit: Option<(egui::Pos2, String)>,
    pub grid: bool,
    /// Downscaled images for the other cells of a multi-image layout.
    previews: HashMap<i64, Option<(TextureHandle, [usize; 2])>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pan,
    Measure,
    Line,
    Rectangle,
    Ellipse,
    Draw,
    Memo,
    Magnifier,
}

impl Tool {
    /// Tools that draw by dragging from one point to another.
    fn drags(self) -> bool {
        matches!(self, Tool::Measure | Tool::Line | Tool::Rectangle | Tool::Ellipse | Tool::Draw)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Line([egui::Pos2; 2]),
    Rectangle([egui::Pos2; 2]),
    Ellipse([egui::Pos2; 2]),
    Draw(Vec<egui::Pos2>),
    Memo { at: egui::Pos2, text: String },
}

/// Rows × columns for a layout index of `settings::LAYOUTS` (1x1, 1x2, 2x1, 2x2).
pub fn layout_grid(index: usize) -> (usize, usize) {
    match index {
        1 => (1, 2),
        2 => (2, 1),
        3 => (2, 2),
        _ => (1, 1),
    }
}

/// Splits `rect` into `rows × cols` cells with a gap, row by row.
pub fn cells(rect: Rect, (rows, cols): (usize, usize), gap: f32) -> Vec<Rect> {
    let w = (rect.width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let h = (rect.height() - gap * (rows as f32 - 1.0)) / rows as f32;
    (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (r, c)))
        .map(|(r, c)| Rect::from_min_size(rect.min + vec2(c as f32 * (w + gap), r as f32 * (h + gap)), vec2(w, h)))
        .collect()
}

fn rgba(c: lumodent::settings::Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn pen(style: lumodent::settings::Stroke) -> egui::Stroke {
    egui::Stroke::new(style.thickness as f32, rgba(style.color))
}

pub fn color_image(width: usize, height: usize, pixels: &[u32]) -> ColorImage {
    let rgb: Vec<u8> = pixels.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    ColorImage::from_rgb([width, height], &rgb)
}

/// Nearest-neighbour reduction for list thumbnails.
pub fn thumbnail(image: &Image, max: usize) -> ColorImage {
    let scale = (max as f32 / image.width.max(image.height) as f32).min(1.0);
    let (w, h) = (((image.width as f32 * scale) as usize).max(1), ((image.height as f32 * scale) as usize).max(1));
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            pixels.push(image.pixels[(y * image.height / h) * image.width + x * image.width / w]);
        }
    }
    color_image(w, h, &pixels)
}

impl ViewerState {
    pub fn show(&mut self, image: Option<Image>, message: &str) {
        self.image = image;
        self.viewport = Viewport::default();
        self.measures.clear();
        self.marks.clear();
        self.stroke.clear();
        self.memo_edit = None;
        self.message = message.into();
        self.refresh();
    }

    /// Rebuilds the displayed pixels after an edit or a tone change.
    pub fn refresh(&mut self) {
        self.display = self
            .image
            .as_ref()
            .map(|i| i.adjusted(&levels(self.brightness, self.contrast)))
            .unwrap_or_default();
        self.texture_stale = true;
    }

    pub fn edit(&mut self, f: impl FnOnce(&mut Image)) {
        if let Some(image) = &mut self.image {
            f(image);
            self.viewport = Viewport::default();
            self.measures.clear();
            self.marks.clear();
            self.refresh();
        }
    }

    /// AutoBrightnessAndContrast: moves the sliders (ratio 0.97, limits ±100).
    pub fn auto_level(&mut self) {
        if let Some(image) = &self.image {
            (self.brightness, self.contrast) = processing::auto_brightness_contrast(image, self.brightness, self.contrast, 100, 0.97);
            self.refresh();
        }
    }

    /// The image as displayed, with tone changes applied, for Send to Report.
    pub fn snapshot(&self) -> Option<Image> {
        let image = self.image.as_ref()?;
        Some(Image { width: image.width, height: image.height, pixels: self.display.clone() })
    }
}

impl App {
    pub fn enter_viewer(&mut self, patient: Patient) -> Result<(), String> {
        if self.viewer.patient.as_ref().map(|p| p.pk) != Some(patient.pk) {
            self.viewer = ViewerState { patient: Some(patient), ..Default::default() };
        }
        self.load_studies(None)
    }

    pub fn load_studies(&mut self, select: Option<i64>) -> Result<(), String> {
        let Some(patient) = self.viewer.patient.clone() else { return Ok(()) };
        let keep = self.viewer.current.and_then(|i| self.viewer.studies.get(i)).map(|s| s.pk);
        self.viewer.studies = self.ws.studies(&patient)?;
        let wanted = select.or(keep);
        let index = wanted
            .and_then(|pk| self.viewer.studies.iter().position(|s| s.pk == pk))
            .or(self.viewer.studies.len().checked_sub(1));
        if index.is_some() && (index != self.viewer.current || select.is_some() || self.viewer.image.is_none()) {
            self.open_study(index.unwrap());
        } else if index.is_none() {
            self.viewer.current = None;
            self.viewer.show(None, t("No images for this patient. Use Import Image to add one."));
        }
        Ok(())
    }

    pub fn open_study(&mut self, index: usize) {
        let Some(study) = self.viewer.studies.get(index) else { return };
        self.viewer.current = Some(index);
        match self.ws.load_study_image(study) {
            Ok((image, spacing)) => {
                self.viewer.show(Some(image), "");
                self.viewer.spacing = spacing;
            }
            Err(e) => {
                self.viewer.show(None, &e);
                self.viewer.spacing = None;
            }
        }
    }

    pub fn import_image(&mut self) -> Result<(), String> {
        let Some(patient) = self.viewer.patient.clone() else { return Ok(()) };
        let Some(path) = (self.hooks.pick_image)() else { return Ok(()) };
        let study = self.ws.import_image(&patient, &path)?;
        self.load_studies(Some(study.pk))?;
        // A DICOM file names its own patient; warn when it is someone else.
        if study.image_type == "dcm" {
            let other = std::fs::read(&path).ok().and_then(|b| lumodent::dicom::parse(&b).ok()).map(|d| d.tags.patient_id);
            if let Some(id) = other.filter(|id| !id.is_empty() && *id != patient.id) {
                self.notice = Some(super::i18n::pick(format!("The DICOM file belongs to patient ID {id}, not {}. It was imported for {} anyway.", patient.id, patient.id), format!("Berkas DICOM ini milik pasien dengan ID {id}, bukan {}. Gambar tetap diimpor untuk {}.", patient.id, patient.id)));
            }
        }
        Ok(())
    }

    pub fn delete_image(&mut self, study: &Study) -> Result<(), String> {
        self.viewer.thumbs.remove(&study.pk);
        let removed = self.ws.remove_study(study);
        self.viewer.current = None;
        self.viewer.image = None;
        self.load_studies(None)?;
        removed
    }

    /// Saves the image as displayed (with tone changes) as PNG or DICOM ("png", "dcm").
    pub fn export_image(&mut self, ext: &str) -> Result<(), String> {
        let Some(image) = self.viewer.snapshot() else { return Ok(()) };
        let stem = self.viewer.current.and_then(|i| self.viewer.studies.get(i)).map(|s| s.image_name.clone()).unwrap_or_else(|| "image".into());
        let stem = std::path::Path::new(&stem).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(stem);
        let Some(path) = (self.hooks.save_file)(&format!("{stem}.{ext}"), ext) else { return Ok(()) };
        if ext == "dcm" {
            let (Some(patient), Some(study)) = (self.viewer.patient.clone(), self.viewer.current.and_then(|i| self.viewer.studies.get(i)).cloned()) else { return Ok(()) };
            std::fs::write(&path, lumodent::dicom::write(&image, &patient, &study)).map_err(|e| format!("Fail to save the image: {e}"))?;
        } else {
            image.save_png(&path)?;
        }
        self.notice = Some(super::i18n::pick(format!("Image saved to {}.", path.display()), format!("Gambar disimpan ke {}.", path.display())));
        Ok(())
    }

    /// Keyboard shortcuts for the viewer canvas.
    fn viewer_keys(&mut self, ui: &Ui, canvas: Rect) {
        if self.viewer.image.is_none() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        use egui::Key;
        let pressed = |k: Key| ui.input(|i| i.key_pressed(k));
        let center = (canvas.size() / 2.0).to_pos2();
        if pressed(Key::Plus) || pressed(Key::Equals) { self.zoom(1.25, canvas, center); }
        if pressed(Key::Minus) { self.zoom(0.8, canvas, center); }
        if pressed(Key::M) { self.viewer.tool = if self.viewer.tool == Tool::Measure { Tool::Pan } else { Tool::Measure }; }
        if pressed(Key::G) { self.viewer.grid = !self.viewer.grid; }
        if pressed(Key::Escape) { self.viewer.tool = Tool::Pan; self.viewer.memo_edit = None; }
        if pressed(Key::R) { self.viewer.edit(Image::rotate_clockwise); }
        if pressed(Key::L) { self.viewer.edit(Image::rotate_counter_clockwise); }
        if pressed(Key::H) { self.viewer.edit(Image::flip_horizontal); }
        if pressed(Key::V) { self.viewer.edit(Image::flip_vertical); }
        if pressed(Key::I) { self.viewer.edit(processing::invert); }
        if pressed(Key::F) || pressed(Key::Num0) { self.viewer.viewport = Viewport::default(); }
        let step = if pressed(Key::ArrowDown) { 1 } else if pressed(Key::ArrowUp) { -1 } else { 0 };
        if step != 0 && !self.viewer.studies.is_empty() {
            let last = self.viewer.studies.len() as i64 - 1;
            let next = (self.viewer.current.map_or(0, |c| c as i64 + step)).clamp(0, last) as usize;
            if Some(next) != self.viewer.current { self.open_study(next); }
        }
    }

    pub fn send_to_report(&mut self) {
        let Some(patient) = self.viewer.patient.clone() else { return };
        let Some(image) = self.viewer.snapshot() else {
            self.notice = Some(t("Error with send image to report, please retry.").into());
            return;
        };
        if self.report.draft.patient.as_ref().map(|p| p.pk) != Some(patient.pk) {
            self.report.draft = Draft::for_patient(&patient);
        }
        self.report.draft.images.push(image);
        self.report.draft.dirty = true;
        self.report.changed();
        let n = self.report.draft.images.len();
        self.notice = Some(super::i18n::pick(format!("Image sent to report ({n} in the current report)."), format!("Gambar dikirim ke laporan ({n} di laporan ini).")));
    }

    fn thumb(&mut self, ctx: &egui::Context, index: usize) -> Option<egui::TextureId> {
        let study = self.viewer.studies.get(index)?.clone();
        let entry = self.viewer.thumbs.entry(study.pk).or_insert_with(|| {
            self.ws
                .load_image(&study)
                .ok()
                .map(|image| ctx.load_texture(format!("thumb-{}", study.pk), thumbnail(&image, 128), TextureOptions::LINEAR))
        });
        entry.as_ref().map(|t| t.id())
    }

    pub(super) fn viewer_screen(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::left("viewer-left")
            .exact_size(self.left_width)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(ui, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(ui.available_height());
                    section(ui, t("IMAGES"));
                    let width = ui.available_width();
                    let mut import = false;
                    let mut delete = false;
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        delete = icons::button(ui, Some(Icon::Delete), t("Delete Image"), vec2(width, 32.0), IconLayout::Inline, self.viewer.current.is_some(), false).clicked();
                        import = icons::button(ui, Some(Icon::Import), t("Import Image"), vec2(width, 32.0), IconLayout::Inline, true, false).clicked();
                        ui.add_space(4.0);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                let mut chosen = None;
                                for i in 0..self.viewer.studies.len() {
                                    let thumb = self.thumb(&ctx, i);
                                    let s = &self.viewer.studies[i];
                                    let when = s.datetime.get(5..16).unwrap_or(&s.datetime).to_string();
                                    let label = format!("{}.  {when}", i + 1);
                                    let selected = self.viewer.current == Some(i);
                                    if list_row(ui, &label, &[(1.0, &label)], 64.0, selected, false, thumb, vec2(64.0, 48.0)).clicked() {
                                        chosen = Some(i);
                                    }
                                }
                                if let Some(i) = chosen {
                                    self.open_study(i);
                                }
                            });
                        });
                    });
                    if import {
                        let result = self.import_image();
                        self.check(result);
                    }
                    if delete {
                        if let Some(study) = self.viewer.current.and_then(|i| self.viewer.studies.get(i)).cloned() {
                            self.ask(t("The selected image will be deleted, are you sure to do so?"), Pending::DeleteImage(study));
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(ui, |ui| {
                let has_image = self.viewer.image.is_some();
                ui.horizontal(|ui| {
                    let size = vec2(82.0, 58.0);
                    let tools: [(Icon, &str, fn(&mut Image)); 5] = [
                        (Icon::RotateLeft, t("Rotate Left"), Image::rotate_counter_clockwise),
                        (Icon::Rotate, t("Rotate Right"), Image::rotate_clockwise),
                        (Icon::FlipH, t("Flip Horizontal"), Image::flip_horizontal),
                        (Icon::FlipV, t("Flip Vertical"), Image::flip_vertical),
                        (Icon::Invert, t("Invert"), processing::invert),
                    ];
                    for (icon, label, f) in tools {
                        if icons::button(ui, Some(icon), label, size, IconLayout::Stacked, has_image, false).clicked() {
                            self.viewer.edit(f);
                        }
                    }
                    let filters = icons::button(ui, Some(Icon::Filter), t("Filters"), size, IconLayout::Stacked, has_image, false);
                    egui::Popup::menu(&filters).show(|ui| {
                        ui.set_min_width(170.0);
                        // Auto sets the Brightness/Contrast sliders, like the reference.
                        if ui.button(t("Auto Level")).clicked() {
                            self.viewer.auto_level();
                        }
                        // Parameters are the ones NanoPix.exe passes to CoreLibrary.dll.
                        let items: [(&str, fn(&mut Image)); 5] = [
                            ("Enhance Detail", processing::enhance),
                            ("Sharpen", |i| {
                                processing::sharpen(i, 1, 0.6, 1.0);
                                processing::median(i, 1);
                            }),
                            ("Smooth", |i| processing::smooth(i, 2, 4.0)),
                            ("Reduce Noise", |i| processing::median(i, 1)),
                            ("Normalize", processing::normalize),
                        ];
                        for (label, f) in items {
                            if ui.button(t(label)).clicked() {
                                self.viewer.edit(f);
                            }
                        }
                    });
                    if icons::button(ui, Some(Icon::Fit), t("Fit"), size, IconLayout::Stacked, has_image, false).clicked() {
                        self.viewer.viewport = Viewport::default();
                    }
                    if icons::button(ui, Some(Icon::Send), t("Send to Report"), size, IconLayout::Stacked, has_image, false).clicked() {
                        self.send_to_report();
                    }
                    if icons::button(ui, Some(Icon::ExportImage), t("Export Image"), size, IconLayout::Stacked, has_image, false).clicked() {
                        let saved = self.export_image("png");
                        self.check(saved);
                    }
                    if icons::button(ui, Some(Icon::ExportImage), t("Export DICOM"), size, IconLayout::Stacked, has_image, false).clicked() {
                        let saved = self.export_image("dcm");
                        self.check(saved);
                    }
                });
                ui.add_space(6.0);
                let bar = 44.0;
                let canvas_size = vec2(ui.available_width(), (ui.available_height() - bar - 6.0).max(80.0));
                let canvas = self.canvas(ui, canvas_size);
                self.viewer_keys(ui, canvas);
                ui.add_space(6.0);
                theme::panel_frame().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let center = canvas.size() / 2.0;
                        for (icon, label, factor) in [(Icon::ZoomOut, t("Zoom Out"), 0.8), (Icon::ZoomIn, t("Zoom In"), 1.25)] {
                            if icons::button(ui, Some(icon), label, vec2(34.0, 30.0), IconLayout::IconOnly, has_image, false).clicked() {
                                self.zoom(factor, canvas, center.to_pos2());
                            }
                        }
                        ui.add_space(8.0);
                        let tools = [
                            (Tool::Pan, Icon::Hand, t("Pan")),
                            (Tool::Measure, Icon::Measure, t("Measure")),
                            (Tool::Line, Icon::Line, t("Line")),
                            (Tool::Rectangle, Icon::Rectangle, t("Rectangle")),
                            (Tool::Ellipse, Icon::Ellipse, t("Ellipse")),
                            (Tool::Draw, Icon::Pencil, t("Free Draw")),
                            (Tool::Memo, Icon::Memo, t("Memo")),
                            (Tool::Magnifier, Icon::Magnifier, t("Magnifier")),
                        ];
                        let small = vec2(34.0, 30.0);
                        for (tool, icon, label) in tools {
                            let on = self.viewer.tool == tool;
                            if icons::button(ui, Some(icon), label, small, IconLayout::IconOnly, has_image, on).clicked() {
                                // Clicking the active tool goes back to panning.
                                self.viewer.tool = if on { Tool::Pan } else { tool };
                                self.viewer.stroke.clear();
                            }
                        }
                        let grid = self.viewer.grid;
                        if icons::button(ui, Some(Icon::Grid), t("Grid"), small, IconLayout::IconOnly, has_image, grid).clicked() {
                            self.viewer.grid = !grid;
                        }
                        let layout = icons::button(ui, Some(Icon::Layout), t("Image Layout"), small, IconLayout::IconOnly, true, self.settings.layout != 0);
                        egui::Popup::menu(&layout).show(|ui| {
                            for (i, name) in lumodent::settings::LAYOUTS.iter().enumerate() {
                                if ui.radio(self.settings.layout == i, *name).clicked() {
                                    self.settings.layout = i;
                                    let saved = self.settings.save(&self.ws.data.join("settings.txt"));
                                    self.check(saved);
                                }
                            }
                        });
                        let marked = !self.viewer.marks.is_empty() || !self.viewer.measures.is_empty();
                        if icons::button(ui, Some(Icon::Clear), t("Clear Annotations"), small, IconLayout::IconOnly, marked, false).clicked() {
                            self.viewer.marks.clear();
                            self.viewer.measures.clear();
                        }
                        ui.add_space(12.0);
                        let mut changed = false;
                        let caption = ui.label(t("Brightness"));
                        changed |= ui.add(egui::Slider::new(&mut self.viewer.brightness, -100..=100).show_value(false)).labelled_by(caption.id).changed();
                        ui.add_space(12.0);
                        let caption = ui.label(t("Contrast"));
                        changed |= ui.add(egui::Slider::new(&mut self.viewer.contrast, -100..=100).show_value(false)).labelled_by(caption.id).changed();
                        ui.add_space(12.0);
                        let tone = self.viewer.brightness != 0 || self.viewer.contrast != 0;
                        if icons::button(ui, Some(Icon::Reset), t("Reset"), vec2(96.0, 30.0), IconLayout::Inline, has_image && tone, false).clicked() {
                            (self.viewer.brightness, self.viewer.contrast) = (0, 0);
                            changed = true;
                        }
                        if changed {
                            self.viewer.refresh();
                        }
                    });
                });
            });
    }

    fn zoom(&mut self, factor: f32, canvas: Rect, at: egui::Pos2) {
        if let Some(i) = &self.viewer.image {
            let size = (canvas.width() as i32, canvas.height() as i32);
            self.viewer.viewport.zoom_at(factor, (i.width, i.height), size, (at.x, at.y));
        }
    }

    /// The image area: one cell per image of the chosen layout. The first cell
    /// holds the open image with every tool; the others preview the next
    /// images, and clicking one opens it.
    fn canvas(&mut self, ui: &mut Ui, size: egui::Vec2) -> Rect {
        let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let grid = layout_grid(self.settings.layout);
        let rects = cells(area, grid, 6.0);
        let first = self.viewer.current.unwrap_or(0);
        for (k, cell) in rects.iter().enumerate().skip(1) {
            self.preview_cell(ui, *cell, first + k);
        }
        self.image_cell(ui, rects[0])
    }

    fn preview(&mut self, ctx: &egui::Context, index: usize) -> Option<(egui::TextureId, [usize; 2])> {
        let study = self.viewer.studies.get(index)?.clone();
        let entry = self.viewer.previews.entry(study.pk).or_insert_with(|| {
            self.ws.load_image(&study).ok().map(|image| {
                let pixels = thumbnail(&image, 1024);
                let size = pixels.size;
                (ctx.load_texture(format!("preview-{}", study.pk), pixels, TextureOptions::LINEAR), size)
            })
        });
        entry.as_ref().map(|(t, s)| (t.id(), *s))
    }

    fn preview_cell(&mut self, ui: &mut Ui, rect: Rect, index: usize) {
        let response = ui.interact(rect, ui.scope_id().with(("preview", index)), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::canvas());
        match self.preview(ui.ctx(), index) {
            Some((texture, [w, h])) => {
                let scale = (rect.width() / w as f32).min(rect.height() / h as f32);
                let dest = Rect::from_center_size(rect.center(), vec2(w as f32 * scale, h as f32 * scale));
                painter.image(texture, dest, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                painter.text(rect.left_top() + vec2(8.0, 6.0), egui::Align2::LEFT_TOP, format!("{}", index + 1), egui::FontId::proportional(13.0), theme::muted());
                if response.hovered() {
                    painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, theme::muted()), egui::StrokeKind::Inside);
                }
                if response.clicked() {
                    self.open_study(index);
                }
            }
            None => {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, t("No image"), egui::FontId::proportional(13.0), theme::muted());
            }
        }
    }

    fn image_cell(&mut self, ui: &mut Ui, rect: Rect) -> Rect {
        let response = ui.interact(rect, ui.scope_id().with("image-cell"), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::canvas());
        let Some(image) = &self.viewer.image else {
            let text = if self.viewer.message.is_empty() { t("No image selected.") } else { &self.viewer.message };
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(15.0), theme::muted());
            return rect;
        };
        let (iw, ih) = (image.width, image.height);
        let (x, y, scale) = self.viewer.viewport.place((iw, ih), (rect.width() as i32, rect.height() as i32));
        let origin = rect.min + vec2(x, y);
        let to_image = |p: egui::Pos2| pos2(((p.x - origin.x) / scale).clamp(0.0, iw as f32), ((p.y - origin.y) / scale).clamp(0.0, ih as f32));
        let tool = self.viewer.tool;
        if tool.drags() {
            if response.drag_started() {
                if let Some(p) = response.interact_pointer_pos() {
                    let q = to_image(p);
                    self.viewer.stroke = vec![q, q];
                }
            }
            if let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged() && !self.viewer.stroke.is_empty()) {
                let q = to_image(p);
                let s = &mut self.viewer.stroke;
                if tool == Tool::Draw {
                    if s.last().is_some_and(|l| l.distance(q) * scale > 2.0) {
                        s.push(q);
                    }
                } else {
                    s[1] = q;
                }
            }
            if response.drag_stopped() {
                let s = std::mem::take(&mut self.viewer.stroke);
                let span = s.first().zip(s.last()).map_or(0.0, |(a, b)| a.distance(*b) * scale);
                if span > 3.0 || (tool == Tool::Draw && s.len() > 2) {
                    let pair = [s[0], s[s.len() - 1]];
                    match tool {
                        Tool::Measure => self.viewer.measures.push(pair),
                        Tool::Line => self.viewer.marks.push(Mark::Line(pair)),
                        Tool::Rectangle => self.viewer.marks.push(Mark::Rectangle(pair)),
                        Tool::Ellipse => self.viewer.marks.push(Mark::Ellipse(pair)),
                        Tool::Draw => self.viewer.marks.push(Mark::Draw(s)),
                        _ => {}
                    }
                }
            }
        } else if tool == Tool::Memo {
            if response.clicked() && self.viewer.memo_edit.is_none() {
                if let Some(p) = response.interact_pointer_pos() {
                    self.viewer.memo_edit = Some((to_image(p), String::new()));
                }
            }
        } else if response.dragged() {
            let d = response.drag_delta();
            self.viewer.viewport.pan.0 += d.x;
            self.viewer.viewport.pan.1 += d.y;
        }
        // Right-click removes the last ruler in Measure mode, else the last annotation.
        if response.secondary_clicked() {
            if tool == Tool::Measure {
                self.viewer.measures.pop();
            } else {
                self.viewer.marks.pop();
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(pointer) = response.hover_pos() {
                    let at = pointer - rect.min;
                    self.zoom(1.0015f32.powf(scroll), rect, at.to_pos2());
                }
            }
        }
        if self.viewer.texture_stale || self.viewer.texture.is_none() {
            let pixels = color_image(iw, ih, &self.viewer.display);
            match &mut self.viewer.texture {
                Some(t) => t.set(pixels, TextureOptions::LINEAR),
                None => self.viewer.texture = Some(ui.ctx().load_texture("viewer", pixels, TextureOptions::LINEAR)),
            }
            self.viewer.texture_stale = false;
        }
        let (x, y, scale) = self.viewer.viewport.place((iw, ih), (rect.width() as i32, rect.height() as i32));
        let dest = Rect::from_min_size(rect.min + vec2(x, y), vec2(iw as f32 * scale, ih as f32 * scale));
        let full_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        if let Some(t) = &self.viewer.texture {
            painter.image(t.id(), dest, full_uv, Color32::WHITE);
        }
        let to_screen = |q: egui::Pos2| dest.min + q.to_vec2() * scale;
        // Millimetres per pixel: the image's own spacing, else the calibration setting.
        let mm_per_px = self.settings.mm_per_pixel(self.viewer.spacing);
        let st = &self.settings;

        if self.viewer.grid {
            use lumodent::settings::{GRID_MM, GRID_OPACITY};
            let step = GRID_MM[st.grid_mm] as f64 / mm_per_px * scale as f64;
            if step >= 4.0 {
                let alpha = (GRID_OPACITY[st.grid_opacity] as f32 / 100.0 * 255.0) as u8;
                let [r, g, b, _] = st.grid.color;
                let line = egui::Stroke::new(st.grid.thickness as f32, Color32::from_rgba_unmultiplied(r, g, b, alpha));
                let mut gx = dest.left() as f64;
                while gx <= dest.right() as f64 {
                    painter.line_segment([pos2(gx as f32, dest.top()), pos2(gx as f32, dest.bottom())], line);
                    gx += step;
                }
                let mut gy = dest.top() as f64;
                while gy <= dest.bottom() as f64 {
                    painter.line_segment([pos2(dest.left(), gy as f32), pos2(dest.right(), gy as f32)], line);
                    gy += step;
                }
            }
        }

        let preview_mark = match (tool, self.viewer.stroke.as_slice()) {
            (Tool::Line, [a, b]) => Some(Mark::Line([*a, *b])),
            (Tool::Rectangle, [a, b]) => Some(Mark::Rectangle([*a, *b])),
            (Tool::Ellipse, [a, b]) => Some(Mark::Ellipse([*a, *b])),
            (Tool::Draw, s) if s.len() > 1 => Some(Mark::Draw(s.to_vec())),
            _ => None,
        };
        for mark in self.viewer.marks.iter().chain(preview_mark.iter()) {
            match mark {
                Mark::Line([a, b]) => {
                    painter.line_segment([to_screen(*a), to_screen(*b)], pen(st.line));
                }
                Mark::Rectangle([a, b]) => {
                    painter.rect_stroke(Rect::from_two_pos(to_screen(*a), to_screen(*b)), 0.0, pen(st.rectangle), egui::StrokeKind::Middle);
                }
                Mark::Ellipse([a, b]) => {
                    let r = Rect::from_two_pos(to_screen(*a), to_screen(*b));
                    painter.add(egui::epaint::EllipseShape::stroke(r.center(), r.size() / 2.0, pen(st.ellipse)));
                }
                Mark::Draw(points) => {
                    painter.add(egui::Shape::line(points.iter().map(|q| to_screen(*q)).collect(), pen(st.draw)));
                }
                Mark::Memo { at, text } => {
                    let galley = painter.layout(text.clone(), egui::FontId::proportional(14.0), rgba(st.memo_text), 320.0);
                    let at = to_screen(*at);
                    let bg = Rect::from_min_size(at, galley.size()).expand(5.0);
                    painter.rect_filled(bg, 4.0, rgba(st.memo_outer));
                    painter.galley(at, galley, rgba(st.memo_text));
                }
            }
        }

        let style = st.length;
        let show_unit = st.length_unit;
        let measuring = if tool == Tool::Measure { self.viewer.stroke.first().zip(self.viewer.stroke.last()).map(|(a, b)| [*a, *b]) } else { None };
        for line in self.viewer.measures.iter().chain(measuring.iter()) {
            let (a, b) = (to_screen(line[0]), to_screen(line[1]));
            let ruler = rgba(style.color);
            let width = style.thickness as f32;
            painter.line_segment([a, b], egui::Stroke::new(width + 1.5, Color32::from_black_alpha(160)));
            painter.line_segment([a, b], egui::Stroke::new(width, ruler));
            painter.circle_filled(a, width + 1.5, ruler);
            painter.circle_filled(b, width + 1.5, ruler);
            let mm = line[0].distance(line[1]) as f64 * mm_per_px;
            let text = if show_unit { format!("{mm:.1} mm") } else { format!("{mm:.1}") };
            let at = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0) + vec2(8.0, -8.0);
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), ruler);
            let bg = egui::Rect::from_min_size(at - vec2(0.0, galley.size().y), galley.size()).expand(3.0);
            painter.rect_filled(bg, 3.0, Color32::from_black_alpha(170));
            painter.galley(bg.min + vec2(3.0, 3.0), galley, ruler);
        }

        // Magnifier: a square lens under the pointer showing the image enlarged.
        if tool == Tool::Magnifier {
            if let (Some(p), Some(t)) = (response.hover_pos(), &self.viewer.texture) {
                use lumodent::settings::{MAGNIFIER_RATIOS, MAGNIFIER_SIZES};
                let side = MAGNIFIER_SIZES[st.magnifier_size] as f32;
                let ratio = MAGNIFIER_RATIOS[st.magnifier_ratio];
                let q = to_image(p);
                let half = side / 2.0 / (scale * ratio);
                let uv = Rect::from_min_max(pos2((q.x - half) / iw as f32, (q.y - half) / ih as f32), pos2((q.x + half) / iw as f32, (q.y + half) / ih as f32));
                let lens = Rect::from_center_size(p, vec2(side, side));
                painter.rect_filled(lens, 0.0, theme::canvas());
                painter.image(t.id(), lens, uv, Color32::WHITE);
                painter.rect_stroke(lens, 0.0, egui::Stroke::new(2.0, rgba(st.capture_border)), egui::StrokeKind::Outside);
            }
        }

        let cursor = match tool {
            Tool::Pan => None,
            Tool::Memo => Some(egui::CursorIcon::Text),
            Tool::Magnifier => Some(egui::CursorIcon::None),
            _ => Some(egui::CursorIcon::Crosshair),
        };
        if let (Some(c), true) = (cursor, response.hovered()) {
            ui.ctx().set_cursor_icon(c);
        }
        painter.text(
            rect.right_bottom() - vec2(8.0, 6.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{iw} × {ih}   {:.0}%", scale * 100.0),
            egui::FontId::proportional(12.0),
            theme::muted(),
        );
        let memo_at = self.viewer.memo_edit.as_ref().map(|(q, _)| dest.min + q.to_vec2() * scale);
        self.memo_editor(ui.ctx(), memo_at);
        rect
    }

    /// The memo box: a text field, the preset texts from Settings, OK and Cancel.
    fn memo_editor(&mut self, ctx: &egui::Context, at: Option<egui::Pos2>) {
        let (Some(at), Some((_, text))) = (at, self.viewer.memo_edit.as_mut()) else { return };
        let presets: Vec<String> = self.settings.memo_presets.iter().filter(|p| !p.trim().is_empty()).cloned().collect();
        let mut done = None;
        egui::Area::new(egui::Id::unique("memo-editor")).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
            theme::panel_frame().show(ui, |ui| {
                ui.set_width(260.0);
                let caption = ui.label(t("Memo"));
                let edit = ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(250.0)).labelled_by(caption.id);
                if text.is_empty() && !edit.has_focus() {
                    edit.request_focus();
                }
                if !presets.is_empty() {
                    ui.label(egui::RichText::new(t("Quick Text")).small().weak());
                    ui.horizontal_wrapped(|ui| {
                        for p in &presets {
                            if ui.small_button(p).clicked() {
                                if !text.is_empty() && !text.ends_with(' ') {
                                    text.push(' ');
                                }
                                text.push_str(p);
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button(t("Add Memo")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(t("Cancel")).clicked() {
                        done = Some(false);
                    }
                });
            });
        });
        match done {
            Some(true) => {
                let (at, text) = self.viewer.memo_edit.take().unwrap();
                if !text.trim().is_empty() {
                    self.viewer.marks.push(Mark::Memo { at, text: text.trim().to_string() });
                }
            }
            Some(false) => self.viewer.memo_edit = None,
            None => {}
        }
    }
}
