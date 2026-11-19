//! The LumoDent window, built with egui so the same code runs on Windows,
//! Linux and macOS. Screens follow the reference application: Patient,
//! Acquisition (not yet available), Viewer and Report.

pub mod icons;
mod patient;
mod report;
pub mod theme;
mod viewer;
#[cfg(test)]
mod tests;

use eframe::egui::{self, vec2, Align, Color32, Id, Layout, Margin, RichText, Ui};
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
    pub save_pdf: Box<dyn FnMut(&str) -> Option<PathBuf>>,
    pub open_file: Box<dyn FnMut(&Path) -> Result<(), String>>,
}

impl Hooks {
    pub fn native() -> Self {
        Self {
            pick_image: Box::new(|| {
                rfd::FileDialog::new()
                    .set_title("Import Image")
                    .add_filter("Image Files (.jpg .jpeg .png .bmp)", &lumodent::workspace::IMPORT_EXTENSIONS)
                    .pick_file()
            }),
            save_pdf: Box::new(|name| {
                rfd::FileDialog::new()
                    .set_title("Export PDF")
                    .add_filter("PDF", &["pdf"])
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
                    self.notice = Some("Please select a patient.".into());
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
        ui.painter().rect_filled(ui.max_rect(), 0.0, theme::BACK);
        egui::Panel::top("menu")
            .frame(egui::Frame::new().fill(theme::EDGE).inner_margin(Margin::symmetric(6, 2)))
            .show(ui, |ui| self.menu(ui));
        egui::Panel::top("header")
            .exact_size(112.0)
            .frame(egui::Frame::new().fill(theme::BACK).inner_margin(Margin { left: 8, right: 12, top: 6, bottom: 0 }))
            .show(ui, |ui| self.header(ui));
        match self.screen {
            Screen::Patient => self.patient_screen(ui),
            Screen::Viewer => self.viewer_screen(ui),
            Screen::Report => self.report_screen(ui),
        }
        let ctx = ui.ctx().clone();
        self.patient_dialogs(&ctx);
        self.dialogs(&ctx);
    }

    fn menu(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Close").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Tools", |ui| {
                ui.add_enabled(false, egui::Button::new("Settings"));
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.about = true;
                }
            });
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
                        (Icon::Patient, "Patient", Some(Screen::Patient)),
                        (Icon::Acquisition, "Acquisition", None),
                        (Icon::Viewer, "Viewer", Some(Screen::Viewer)),
                        (Icon::Report, "Report", Some(Screen::Report)),
                    ];
                    for (icon, label, target) in tabs {
                        let selected = target == Some(self.screen);
                        let size = vec2(150.0, 46.0);
                        if icons::button(ui, Some(icon), label, size, icons::Layout::Inline, true, selected).clicked() {
                            match target {
                                Some(screen) => self.navigate(screen),
                                None => {
                                    self.notice = Some("The Acquisition module is not available in LumoDent yet.".into())
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
                    .map(|p| format!("{} {}", p.details.last_name, p.details.first_name))
                    .unwrap_or_else(|| "Please select a patient.".into());
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
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.button("OK").clicked()).inner
            });
            if modal.inner || modal.should_close() {
                self.notice = None;
            }
        }
        if let Some(confirm) = &self.confirm {
            let text = confirm.text.clone();
            let modal = egui::Modal::new(Id::unique("confirm")).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.heading("QUESTION");
                ui.add_space(4.0);
                ui.label(text);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let no = ui.button("No").clicked();
                    let yes = ui.button("Yes").clicked();
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
                    ui.label("Intraoral X-ray workstation");
                    ui.label(format!("Developer: {DEVELOPER}"));
                    ui.add_space(8.0);
                    ui.button("OK").clicked()
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
    ui.label(RichText::new(text).size(15.0).color(theme::TEXT));
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
            theme::ACCENT
        } else if response.hovered() {
            theme::BUTTON
        } else if stripe {
            Color32::from_rgb(0x4c, 0x4c, 0x4c)
        } else {
            theme::PANEL
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
                theme::TEXT,
            );
            offset += text_width * fraction;
        }
    }
    response
}
