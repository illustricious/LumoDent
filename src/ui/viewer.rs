//! Viewer screen: the patient's images with thumbnails, image tools, a canvas
//! with drag-to-pan and wheel zoom, and brightness/contrast.

use super::icons::{self, Icon, Layout as IconLayout};
use super::{list_row, section, theme, App, Pending};
use eframe::egui::{self, pos2, vec2, Align, Color32, ColorImage, Layout, Rect, TextureHandle, TextureOptions, Ui};
use lumodent::image::{levels, Image, Viewport};
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
            self.viewer.show(None, "No images for this patient. Use Import Image to add one.");
        }
        Ok(())
    }

    pub fn open_study(&mut self, index: usize) {
        let Some(study) = self.viewer.studies.get(index) else { return };
        self.viewer.current = Some(index);
        match self.ws.load_image(study) {
            Ok(image) => self.viewer.show(Some(image), ""),
            Err(e) => self.viewer.show(None, &e),
        }
    }

    pub fn import_image(&mut self) -> Result<(), String> {
        let Some(patient) = self.viewer.patient.clone() else { return Ok(()) };
        let Some(path) = (self.hooks.pick_image)() else { return Ok(()) };
        let study = self.ws.import_image(&patient, &path)?;
        self.load_studies(Some(study.pk))
    }

    pub fn delete_image(&mut self, study: &Study) -> Result<(), String> {
        self.viewer.thumbs.remove(&study.pk);
        let removed = self.ws.remove_study(study);
        self.viewer.current = None;
        self.viewer.image = None;
        self.load_studies(None)?;
        removed
    }

    pub fn send_to_report(&mut self) {
        let Some(patient) = self.viewer.patient.clone() else { return };
        let Some(image) = self.viewer.snapshot() else {
            self.notice = Some("Error with send image to report, please retry.".into());
            return;
        };
        if self.report.draft.patient.as_ref().map(|p| p.pk) != Some(patient.pk) {
            self.report.draft = Draft::for_patient(&patient);
        }
        self.report.draft.images.push(image);
        self.report.draft.dirty = true;
        self.report.changed();
        self.notice = Some(format!("Image sent to report ({} in the current report).", self.report.draft.images.len()));
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
                    section(ui, "IMAGES");
                    let width = ui.available_width();
                    let mut import = false;
                    let mut delete = false;
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        delete = icons::button(ui, Some(Icon::Delete), "Delete Image", vec2(width, 32.0), IconLayout::Inline, self.viewer.current.is_some(), false).clicked();
                        import = icons::button(ui, Some(Icon::Import), "Import Image", vec2(width, 32.0), IconLayout::Inline, true, false).clicked();
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
                            self.ask("The selected image will be deleted, are you sure to do so?", Pending::DeleteImage(study));
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 8, bottom: 8 }))
            .show(ui, |ui| {
                let has_image = self.viewer.image.is_some();
                ui.horizontal(|ui| {
                    let size = vec2(110.0, 58.0);
                    let tools: [(Icon, &str, fn(&mut Image)); 4] = [
                        (Icon::Rotate, "Rotate", Image::rotate_clockwise),
                        (Icon::FlipH, "Flip Horizontal", Image::flip_horizontal),
                        (Icon::FlipV, "Flip Vertical", Image::flip_vertical),
                        (Icon::Invert, "Invert", Image::invert),
                    ];
                    for (icon, label, f) in tools {
                        if icons::button(ui, Some(icon), label, size, IconLayout::Stacked, has_image, false).clicked() {
                            self.viewer.edit(f);
                        }
                    }
                    if icons::button(ui, Some(Icon::Fit), "Fit", size, IconLayout::Stacked, has_image, false).clicked() {
                        self.viewer.viewport = Viewport::default();
                    }
                    if icons::button(ui, Some(Icon::Send), "Send to Report", size, IconLayout::Stacked, has_image, false).clicked() {
                        self.send_to_report();
                    }
                });
                ui.add_space(6.0);
                let bar = 44.0;
                let canvas_size = vec2(ui.available_width(), (ui.available_height() - bar - 6.0).max(80.0));
                let canvas = self.canvas(ui, canvas_size);
                ui.add_space(6.0);
                theme::panel_frame().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let center = canvas.size() / 2.0;
                        for (icon, label, factor) in [(Icon::ZoomOut, "Zoom Out", 0.8), (Icon::ZoomIn, "Zoom In", 1.25)] {
                            if icons::button(ui, Some(icon), label, vec2(110.0, 30.0), IconLayout::Inline, has_image, false).clicked() {
                                self.zoom(factor, canvas, center.to_pos2());
                            }
                        }
                        ui.add_space(16.0);
                        let mut changed = false;
                        let caption = ui.label("Brightness");
                        changed |= ui.add(egui::Slider::new(&mut self.viewer.brightness, -100..=100).show_value(false)).labelled_by(caption.id).changed();
                        ui.add_space(12.0);
                        let caption = ui.label("Contrast");
                        changed |= ui.add(egui::Slider::new(&mut self.viewer.contrast, -100..=100).show_value(false)).labelled_by(caption.id).changed();
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

    fn canvas(&mut self, ui: &mut Ui, size: egui::Vec2) -> Rect {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::CANVAS);
        let Some(image) = &self.viewer.image else {
            let text = if self.viewer.message.is_empty() { "No image selected." } else { &self.viewer.message };
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(15.0), theme::MUTED);
            return rect;
        };
        let (iw, ih) = (image.width, image.height);
        if response.dragged() {
            let d = response.drag_delta();
            self.viewer.viewport.pan.0 += d.x;
            self.viewer.viewport.pan.1 += d.y;
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
        if let Some(t) = &self.viewer.texture {
            painter.image(t.id(), dest, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        painter.text(
            rect.right_bottom() - vec2(8.0, 6.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{iw} × {ih}   {:.0}%", scale * 100.0),
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );
        rect
    }
}
