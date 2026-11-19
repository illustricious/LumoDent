//! The LumoDent window, built with egui so the same code runs on Windows,
//! Linux and macOS. Screens follow the reference application: Patient,
//! Acquisition (not yet available), Viewer and Report.

pub mod i18n;
pub mod icons;
mod patient;
mod report;
pub mod theme;
mod viewer;
#[cfg(test)]
mod tests;

use eframe::egui::{self, vec2, Align, Color32, Id, Layout, Margin, RichText, Ui};
use i18n::t;
use icons::Icon;
use lumodent::patients::Patient;
use lumodent::studies::{Report, Study};
use lumodent::workspace::Workspace;
use lumodent::{APP_NAME, DEVELOPER};
use std::path::{Path, PathBuf};

pub use patient::PatientState;
pub use report::ReportState;
pub use viewer::ViewerState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Patient,
    Viewer,
    Report,
}

/// An action waiting for the user to answer a QUESTION dialog.
pub enum Pending {
    DeletePatient(Patient),
    DeleteImage(Study),
    DeleteReport(Report),
    DiscardForNewReport,
    DiscardForReport(usize),
}

pub struct Confirm {
    pub text: String,
    pub action: Pending,
}

/// Operating-system services, replaceable in tests.
pub struct Hooks {
    pub pick_image: Box<dyn FnMut() -> Option<PathBuf>>,
    /// Asks where to save a file: suggested name and extension ("pdf", "png").
    pub save_file: Box<dyn FnMut(&str, &str) -> Option<PathBuf>>,
    pub open_file: Box<dyn FnMut(&Path) -> Result<(), String>>,
}

impl Hooks {
    pub fn native() -> Self {
        Self {
            pick_image: Box::new(|| {
                rfd::FileDialog::new()
                    .set_title(t("Import Image"))
                    .add_filter(t("Image Files (.jpg .jpeg .png .bmp .dcm)"), &lumodent::workspace::IMPORT_EXTENSIONS)
                    .pick_file()
            }),
            save_file: Box::new(|name, ext| {
                rfd::FileDialog::new()
                    .set_title(t("Save As"))
                    .add_filter(ext.to_uppercase(), &[ext])
                    .set_file_name(name)
                    .save_file()
            }),
            open_file: Box::new(open_with_system),
        }
    }
}

/// Opens a file in the default application, e.g. a PDF viewer for printing.
pub fn open_with_system(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    command.spawn().map(|_| ()).map_err(|e| format!("Cannot open {}: {e}", path.display()))
}

pub struct App {
    pub ws: Workspace,
    pub screen: Screen,
    pub patients: PatientState,
    pub viewer: ViewerState,
    pub report: ReportState,
    pub notice: Option<String>,
    pub confirm: Option<Confirm>,
    pub about: bool,
    pub hooks: Hooks,
    left_width: f32,
}

impl App {
    pub fn new(ws: Workspace, hooks: Hooks) -> Self {
        let clinic = ws.load_clinic();
        let mut app = Self {
            ws,
            screen: Screen::Patient,
            patients: PatientState::default(),
            viewer: ViewerState::default(),
            report: ReportState::new(clinic),
            notice: None,
            confirm: None,
            about: false,
            hooks,
            left_width: 180.0,
        };
        let loaded = app.refresh_patients();
        app.check(loaded);
        app
    }

    /// Switches between the dark and light palette and remembers the choice.
    pub fn set_theme(&mut self, ctx: &egui::Context, light: bool) {
        theme::set_light(ctx, light);
        let saved = std::fs::write(theme_path(&self.ws.data), if light { "light" } else { "dark" })
            .map_err(|e| format!("Cannot save the theme: {e}"));
        self.check(saved);
    }

    /// Switches the interface language and remembers the choice.
    pub fn set_language(&mut self, indonesian: bool) {
        i18n::set_indonesian(indonesian);
        let saved = std::fs::write(self.ws.data.join("language.txt"), if indonesian { "id" } else { "en" })
            .map_err(|e| format!("Cannot save the language: {e}"));
        self.check(saved);
    }

    /// Shows an error from an operation in the notice dialog.
    pub fn check<T>(&mut self, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.notice = Some(e);
                None
            }
        }
    }

    fn ask(&mut self, text: &str, action: Pending) {
        self.confirm = Some(Confirm { text: text.into(), action });
    }

    /// The patient the current screen is about.
    fn context_patient(&self) -> Option<Patient> {
        match self.screen {
            Screen::Patient => self.patients.selected_patient().cloned(),
            Screen::Viewer => self.viewer.patient.clone(),
            Screen::Report => self.report.draft.patient.clone(),
        }
    }

    fn navigate(&mut self, target: Screen) {
        if target == self.screen {
            return;
        }
        if self.screen == Screen::Report {
            let saved = self.ws.save_clinic(&self.report.clinic);
            self.check(saved);
        }
        match target {
            Screen::Patient => {
                self.screen = Screen::Patient;
                if let Some(pk) = self.context_patient().map(|p| p.pk) {
                    self.patients.selected = Some(pk);
                }
            }
            Screen::Viewer | Screen::Report => {
                let Some(patient) = self.context_patient() else {
                    self.notice = Some(t("Please select a patient.").into());
                    return;
                };
                let entered = if target == Screen::Viewer {
                    self.enter_viewer(patient)
                } else {
                    self.enter_report(patient)
                };
                if self.check(entered).is_some() {
                    self.screen = target;
                }
            }
        }
    }

    fn resolve(&mut self, action: Pending) {
        let result = match action {
            Pending::DeletePatient(p) => self.delete_patient(&p),
            Pending::DeleteImage(study) => self.delete_image(&study),
            Pending::DeleteReport(row) => self.delete_report(&row),
            Pending::DiscardForNewReport => {
                self.reset_draft();
                Ok(())
            }
            Pending::DiscardForReport(index) => self.load_report(index),
        };
        self.check(result);
    }

    /// Draws the whole window into `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        self.left_width = (ui.max_rect().width() * 0.17).clamp(200.0, 260.0);
        ui.painter().rect_filled(ui.max_rect(), 0.0, theme::back());
        egui::Panel::top("menu")
            .frame(egui::Frame::new().fill(theme::edge()).inner_margin(Margin { left: 6, right: 0, top: 0, bottom: 0 }))
            .show(ui, |ui| self.menu(ui));
        egui::Panel::top("header")
            .exact_size(112.0)
            .frame(egui::Frame::new().fill(theme::back()).inner_margin(Margin { left: 8, right: 12, top: 6, bottom: 0 }))
            .show(ui, |ui| self.header(ui));
        match self.screen {
            Screen::Patient => self.patient_screen(ui),
            Screen::Viewer => self.viewer_screen(ui),
            Screen::Report => self.report_screen(ui),
        }
        let ctx = ui.ctx().clone();
        self.patient_dialogs(&ctx);
        self.dialogs(&ctx);
        resize_edges(&ctx);
    }

    /// The top bar: menus, the window title and the window buttons, in one row
    /// (the window has no system title bar). Empty space drags the window.
    fn menu(&mut self, ui: &mut Ui) {
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        let bar = ui.max_rect();
        let drag = ui.interact(bar, Id::unique("title-drag"), egui::Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        ui.horizontal(|ui| {
            egui::MenuBar::new().ui(ui, |ui| self.menus(ui));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if window_button(ui, WindowButton::Close, t("Close window")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let (kind, tip) = if maximized { (WindowButton::Restore, t("Restore")) } else { (WindowButton::Maximize, t("Maximize")) };
                if window_button(ui, kind, tip).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if window_button(ui, WindowButton::Minimize, t("Minimize")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                ui.painter().text(bar.center(), egui::Align2::CENTER_CENTER, APP_NAME, egui::FontId::proportional(13.0), theme::muted());
            });
        });
    }

    fn menus(&mut self, ui: &mut Ui) {
        let has_patient = self.context_patient().is_some();
        let has_image = self.viewer.image.is_some();
        let in_viewer = self.screen == Screen::Viewer;
        let in_report = self.screen == Screen::Report;
        ui.menu_button(t("File"), |ui| {
            if ui.button(t("New Patient")).clicked() {
                self.navigate(Screen::Patient);
                self.open_editor(false);
            }
            if ui.add_enabled(in_viewer, egui::Button::new(t("Import Image…"))).clicked() {
                let r = self.import_image();
                self.check(r);
            }
            ui.separator();
            if ui.add_enabled(in_viewer && has_image, egui::Button::new(t("Export Image as PNG…"))).clicked() {
                let r = self.export_image("png");
                self.check(r);
            }
            if ui.add_enabled(in_viewer && has_image, egui::Button::new(t("Export Image as DICOM…"))).clicked() {
                let r = self.export_image("dcm");
                self.check(r);
            }
            if ui.add_enabled(in_report, egui::Button::new(t("Export Report as PDF…"))).clicked() {
                let r = self.export_pdf();
                self.check(r);
            }
            if ui.add_enabled(in_report, egui::Button::new(t("Print Report…"))).clicked() {
                let r = self.print_report();
                self.check(r);
            }
            ui.separator();
            if ui.button(t("Exit")).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button(t("View"), |ui| {
            for (label, screen) in [(t("Patient"), Screen::Patient), (t("Viewer"), Screen::Viewer), (t("Report"), Screen::Report)] {
                let enabled = screen == Screen::Patient || has_patient;
                if ui.add_enabled(enabled, egui::RadioButton::new(self.screen == screen, label)).clicked() {
                    self.navigate(screen);
                }
            }
            ui.separator();
            let full = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
            if ui.add(egui::RadioButton::new(full, t("Full Screen"))).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
            }
        });
        ui.menu_button(t("Tools"), |ui| {
            ui.menu_button(t("Theme"), |ui| {
                for (light, label) in [(false, t("Dark")), (true, t("Light"))] {
                    if ui.radio(theme::is_light() == light, label).clicked() {
                        self.set_theme(ui.ctx(), light);
                    }
                }
            });
            ui.menu_button(t("Language"), |ui| {
                for (id, label) in [(false, "English"), (true, "Bahasa Indonesia")] {
                    if ui.radio(i18n::is_indonesian() == id, label).clicked() {
                        self.set_language(id);
                    }
                }
            });
            ui.separator();
            if ui.button(t("Open Data Folder")).clicked() {
                let dir = self.ws.data.clone();
                let r = (self.hooks.open_file)(&dir);
                self.check(r);
            }
            if ui.add_enabled(has_patient, egui::Button::new(t("Open Patient Image Folder"))).clicked() {
                if let Some(p) = self.context_patient() {
                    let dir = self.ws.image_dir(&p.id);
                    let r = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| (self.hooks.open_file)(&dir));
                    self.check(r);
                }
            }
        });
        ui.menu_button(t("Help"), |ui| {
            if ui.button(t("Keyboard Shortcuts")).clicked() {
                self.notice = Some(if i18n::is_indonesian() { SHORTCUTS_ID } else { SHORTCUTS }.into());
            }
            if ui.button(t("About")).clicked() {
                self.about = true;
            }
        });
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            icons::logo(ui, vec2(self.left_width - 8.0, 100.0));
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let tabs = [
                        (Icon::Patient, t("Patient"), Some(Screen::Patient)),
                        (Icon::Acquisition, t("Acquisition"), None),
                        (Icon::Viewer, t("Viewer"), Some(Screen::Viewer)),
                        (Icon::Report, t("Report"), Some(Screen::Report)),
                    ];
                    for (icon, label, target) in tabs {
                        let selected = target == Some(self.screen);
                        let size = vec2(150.0, 46.0);
                        if icons::button(ui, Some(icon), label, size, icons::Layout::Inline, true, selected).clicked() {
                            match target {
                                Some(screen) => self.navigate(screen),
                                None => {
                                    self.notice = Some(t("The Acquisition module is not available in LumoDent yet.").into())
                                }
                            }
                        }
                    }
                });
            });
            let rest = vec2(ui.available_width(), 60.0);
            ui.allocate_ui_with_layout(rest, Layout::right_to_left(Align::Center), |ui| {
                let name = self
                    .context_patient()
                    .map(|p| p.details.full_name())
                    .unwrap_or_else(|| t("Please select a patient.").into());
                ui.label(RichText::new(name).size(16.0));
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(text) = self.notice.clone() {
            let modal = egui::Modal::new(Id::unique("notice")).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.heading(APP_NAME);
                ui.add_space(4.0);
                ui.label(text);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.button(t("OK")).clicked()).inner
            });
            if modal.inner || modal.should_close() {
                self.notice = None;
            }
        }
        if let Some(confirm) = &self.confirm {
            let text = confirm.text.clone();
            let modal = egui::Modal::new(Id::unique("confirm")).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.heading(t("QUESTION"));
                ui.add_space(4.0);
                ui.label(text);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let no = ui.button(t("No")).clicked();
                    let yes = ui.button(t("Yes")).clicked();
                    (yes, no)
                })
                .inner
            });
            let (yes, no) = modal.inner;
            if yes {
                let action = self.confirm.take().unwrap().action;
                self.resolve(action);
            } else if no || modal.should_close() {
                self.confirm = None;
            }
        }
        if self.about {
            let modal = egui::Modal::new(Id::unique("about")).show(ctx, |ui| {
                ui.set_width(320.0);
                icons::logo(ui, vec2(320.0, 100.0));
                ui.vertical_centered(|ui| {
                    ui.label(format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION")));
                    ui.label(t("Intraoral X-ray workstation"));
                    ui.label(format!("{}: {DEVELOPER}", if i18n::is_indonesian() { "Pengembang" } else { "Developer" }));
                    ui.add_space(8.0);
                    ui.button(t("OK")).clicked()
                })
                .inner
            });
            if modal.inner || modal.should_close() {
                self.about = false;
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn on_exit(&mut self) {
        let _ = self.ws.save_clinic(&self.report.clinic);
    }
}

/// A heading line inside a panel, in the reference application's caps style.
pub fn section(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).color(theme::text()));
    ui.add_space(2.0);
}

/// A clickable list row with an optional thumbnail, used by every list.
pub fn list_row(
    ui: &mut Ui,
    label: &str,
    columns: &[(f32, &str)],
    height: f32,
    selected: bool,
    stripe: bool,
    thumb: Option<egui::TextureId>,
    thumb_size: egui::Vec2,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fill = if selected {
            theme::accent()
        } else if response.hovered() {
            theme::button()
        } else if stripe {
            theme::stripe()
        } else {
            theme::panel()
        };
        p.rect_filled(rect, 2.0, fill);
        let mut x = rect.left() + 6.0;
        if let Some(id) = thumb {
            let r = egui::Rect::from_min_size(egui::pos2(x, rect.center().y - thumb_size.y / 2.0), thumb_size);
            p.image(id, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            x = r.right() + 8.0;
        }
        let text_width = rect.right() - x;
        let clip = p.with_clip_rect(rect.shrink(1.0));
        let mut offset = 0.0;
        for (fraction, text) in columns {
            clip.text(
                egui::pos2(x + offset, rect.center().y),
                egui::Align2::LEFT_CENTER,
                *text,
                egui::FontId::proportional(14.0),
                theme::text(),
            );
            offset += text_width * fraction;
        }
    }
    response
}

fn theme_path(data: &Path) -> PathBuf {
    data.join("theme.txt")
}

/// Whether the saved theme is the light palette (dark is the default).
pub fn saved_light(data: &Path) -> bool {
    std::fs::read_to_string(theme_path(data)).is_ok_and(|s| s.trim() == "light")
}

/// Column titles laid out exactly like the cells of `list_row` below them.
pub fn list_header(ui: &mut Ui, columns: &[(f32, &str)]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), egui::Sense::hover());
    let x = rect.left() + 6.0;
    let text_width = rect.right() - x;
    let mut offset = 0.0;
    for (fraction, title) in columns {
        let at = egui::pos2(x + offset, rect.center().y);
        ui.painter().text(at, egui::Align2::LEFT_CENTER, *title, egui::FontId::proportional(14.0), theme::muted());
        offset += text_width * fraction;
    }
}

const SHORTCUTS: &str = "Viewer shortcuts\n\n+ / -   Zoom in / out\nR / L   Rotate right / left\nH / V   Flip horizontal / vertical\nI   Invert\nF or 0   Fit\nUp / Down   Previous / next image\n\nDouble-click the title bar to maximize or restore.";

#[derive(Clone, Copy, PartialEq)]
enum WindowButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// A caption button drawn like the system ones; Close turns red on hover.
fn window_button(ui: &mut Ui, kind: WindowButton, tip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(46.0, 30.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let hovered = response.hovered();
    if hovered {
        let fill = if kind == WindowButton::Close { Color32::from_rgb(0xc4, 0x2b, 0x1c) } else { theme::button_hover() };
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    let color = if hovered && kind == WindowButton::Close { Color32::WHITE } else { theme::text() };
    let s = egui::Stroke::new(1.0, color);
    let c = rect.center();
    let p = ui.painter();
    match kind {
        WindowButton::Minimize => {
            p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], s);
        }
        WindowButton::Maximize => {
            p.rect_stroke(egui::Rect::from_center_size(c, vec2(10.0, 10.0)), 0.0, s, egui::StrokeKind::Middle);
        }
        WindowButton::Restore => {
            p.rect_stroke(egui::Rect::from_center_size(c + vec2(-1.5, 1.5), vec2(8.0, 8.0)), 0.0, s, egui::StrokeKind::Middle);
            p.line_segment([c + vec2(-3.5, -3.5), c + vec2(4.5, -3.5)], s);
            p.line_segment([c + vec2(4.5, -3.5), c + vec2(4.5, 4.5)], s);
        }
        WindowButton::Close => {
            p.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
            p.line_segment([c + vec2(5.0, -5.0), c + vec2(-5.0, 5.0)], s);
        }
    }
    response.on_hover_text(tip)
}

/// Lets the borderless window be resized from its edges and corners.
fn resize_edges(ctx: &egui::Context) {
    use egui::{CursorIcon, ResizeDirection as D};
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if maximized || fullscreen {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let r = ctx.content_rect();
    let m = 5.0;
    let (l, rt, t, b) = (pos.x - r.left() < m, r.right() - pos.x < m, pos.y - r.top() < m, r.bottom() - pos.y < m);
    let dir = match (l, rt, t, b) {
        (true, _, true, _) => D::NorthWest,
        (_, true, true, _) => D::NorthEast,
        (true, _, _, true) => D::SouthWest,
        (_, true, _, true) => D::SouthEast,
        (true, ..) => D::West,
        (_, true, ..) => D::East,
        (_, _, true, _) => D::North,
        (_, _, _, true) => D::South,
        _ => return,
    };
    let cursor = match dir {
        D::NorthWest | D::SouthEast => CursorIcon::ResizeNwSe,
        D::NorthEast | D::SouthWest => CursorIcon::ResizeNeSw,
        D::West | D::East => CursorIcon::ResizeHorizontal,
        D::North | D::South => CursorIcon::ResizeVertical,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
}

/// Whether the saved interface language is Indonesian (English is the default).
pub fn saved_indonesian(data: &Path) -> bool {
    std::fs::read_to_string(data.join("language.txt")).is_ok_and(|s| s.trim() == "id")
}

const SHORTCUTS_ID: &str = "Pintasan Penampil\n\n+ / -   Perbesar / perkecil\nR / L   Putar kanan / kiri\nH / V   Balik horizontal / vertikal\nI   Negatif\nF atau 0   Pas layar\nAtas / Bawah   Gambar sebelumnya / berikutnya\n\nKlik ganda title bar untuk membesarkan atau memulihkan.";
