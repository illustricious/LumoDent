//! Patient screen: search, quick search by recent activity, the patient list,
//! the summary card and the New/Edit Patient form.

use super::icons::{self, Icon, Layout as IconLayout};
use super::{list_row, section, t, theme, App, Pending};
use eframe::egui::{self, vec2, Align, Id, Layout, RichText, Ui};
use lumodent::patients::{Patient, PatientDetails};

pub const FIELDS: [&str; 11] = [
    "Chart No.",
    "First Name",
    "Last Name",
    "Gender",
    "Date of Birth",
    "Doctor",
    "Social ID",
    "Phone",
    "E-Mail",
    "Zip Code",
    "Address",
];

pub const DATES: [&str; 9] = [
    "All Dates",
    "Yesterday",
    "Three Days",
    "This Week",
    "Last Week",
    "This Month",
    "Last Month",
    "Three Months",
    "Six Months",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Search,
    RecentlyAcquired,
    RecentlyPrinted,
}

#[derive(Default)]
pub struct PatientState {
    pub query: String,
    pub date: usize,
    pub scope: Scope,
    pub rows: Vec<Patient>,
    pub selected: Option<i64>,
    pub editor: Option<Editor>,
    pub detail: bool,
}

pub struct Editor {
    pub pk: Option<i64>,
    pub fields: [String; 11],
}

impl PatientState {
    pub fn selected_patient(&self) -> Option<&Patient> {
        let pk = self.selected?;
        self.rows.iter().find(|p| p.pk == pk)
    }
}

fn values(p: &Patient) -> [String; 11] {
    let d = &p.details;
    [
        &p.id, &d.first_name, &d.last_name, &d.sex, &d.birthdate, &d.doctor, &d.social_id, &d.phone, &d.email,
        &d.zipcode, &d.address,
    ]
    .map(|s| s.clone())
}

impl App {
    pub fn refresh_patients(&mut self) -> Result<(), String> {
        let (from, to) = lumodent::dates::date_range(self.patients.date)?;
        let store = &self.ws.store;
        self.patients.rows = match self.patients.scope {
            Scope::Search => store.list_created_between(self.patients.query.trim(), &from, &to)?,
            Scope::RecentlyAcquired => store.recently_acquired(&from, &to)?,
            Scope::RecentlyPrinted => store.recently_printed(&from, &to)?,
        };
        if self.patients.selected_patient().is_none() {
            self.patients.selected = None;
        }
        Ok(())
    }

    fn search(&mut self, scope: Scope) {
        self.patients.scope = scope;
        let result = self.refresh_patients();
        self.check(result);
    }

    pub fn open_editor(&mut self, existing: bool) {
        let editor = if existing {
            let Some(p) = self.patients.selected_patient() else { return };
            Editor { pk: Some(p.pk), fields: values(p) }
        } else {
            Editor { pk: None, fields: Default::default() }
        };
        self.patients.editor = Some(editor);
    }

    pub fn save_editor(&mut self) -> Result<(), String> {
        let Some(editor) = &self.patients.editor else { return Ok(()) };
        let f = editor.fields.clone().map(|s| s.trim().to_string());
        if f[0].is_empty() {
            return Err(t("Patient's Chart Number can't be empty, please input one.").into());
        }
        if f[1].is_empty() && f[2].is_empty() {
            return Err(t("Patient's name can't be empty, please input one.").into());
        }
        let [id, first_name, last_name, sex, birthdate, doctor, social_id, phone, email, zipcode, address] = f;
        let details = PatientDetails { last_name, first_name, sex, birthdate, doctor, social_id, phone, email, zipcode, address };
        let pk = match editor.pk {
            Some(pk) => {
                self.ws.store.update(pk, &details)?;
                pk
            }
            None => self.ws.store.add(&id, &details).map_err(|e| {
                if e.contains("UNIQUE") {
                    format!("Chart No. {id} already exists.")
                } else {
                    e
                }
            })?,
        };
        self.patients.editor = None;
        // Show the saved patient even if the current filter would hide it.
        self.patients.scope = super::patient::Scope::Search;
        self.patients.date = 0;
        self.refresh_patients()?;
        self.patients.selected = Some(pk);
        Ok(())
    }

    pub fn delete_patient(&mut self, patient: &Patient) -> Result<(), String> {
        self.ws.delete_patient(patient)?;
        if self.report.draft.patient.as_ref().map(|p| p.pk) == Some(patient.pk) {
            self.report.draft = Default::default();
            self.report.changed();
        }
        if self.viewer.patient.as_ref().map(|p| p.pk) == Some(patient.pk) {
            self.viewer = Default::default();
        }
        self.patients.selected = None;
        self.refresh_patients()
    }

    pub(super) fn patient_screen(&mut self, ui: &mut Ui) {
        let left = self.left_width;
        egui::Panel::left("patient-left")
            .exact_size(left)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(ui, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section(ui, t("SEARCH"));
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.patients.query)
                            .hint_text("Chart No., name or phone")
                            .desired_width(f32::INFINITY),
                    );
                    field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, t("Search patients")));
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let width = ui.available_width();
                    if icons::button(ui, Some(Icon::Search), t("Search"), vec2(width, 32.0), IconLayout::Inline, true, false).clicked() || enter {
                        self.search(super::patient::Scope::Search);
                    }
                });
                ui.add_space(8.0);
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section(ui, t("QUICK SEARCH"));
                    let width = ui.available_width();
                    for (icon, label, scope) in [
                        (Icon::Clock, t("Recently Acquired"), Scope::RecentlyAcquired),
                        (Icon::Print, t("Recently Printed"), Scope::RecentlyPrinted),
                    ] {
                        let active = self.patients.scope == scope;
                        if icons::button(ui, Some(icon), label, vec2(width, 32.0), IconLayout::Inline, true, active).clicked() {
                            self.search(scope);
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(ui, |ui| {
                let selected = self.patients.selected_patient().is_some();
                ui.horizontal(|ui| {
                    let size = vec2(104.0, 58.0);
                    if icons::button(ui, Some(Icon::AddPatient), t("New Patient"), size, IconLayout::Stacked, true, false).clicked() {
                        self.open_editor(false);
                    }
                    if icons::button(ui, Some(Icon::Edit), t("Edit Patient"), size, IconLayout::Stacked, selected, false).clicked() {
                        self.open_editor(true);
                    }
                    if icons::button(ui, Some(Icon::Delete), t("Delete Patient"), size, IconLayout::Stacked, selected, false).clicked() {
                        if let Some(p) = self.patients.selected_patient().cloned() {
                            self.ask(t("All of the patient's data will be deleted, are you sure to do so?"), Pending::DeletePatient(p));
                        }
                    }
                    if icons::button(ui, Some(Icon::Clear), t("Clear List"), size, IconLayout::Stacked, !self.patients.rows.is_empty(), false).clicked() {
                        self.patients.rows.clear();
                        self.patients.selected = None;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let before = self.patients.date;
                        // Right-to-left: the combo box first, then its caption on its left.
                        let combo = egui::ComboBox::from_id_salt("date-filter")
                            .width(170.0)
                            .selected_text(t(DATES[self.patients.date]))
                            .show_ui(ui, |ui| {
                                for (i, name) in DATES.iter().enumerate() {
                                    ui.selectable_value(&mut self.patients.date, i, t(name));
                                }
                            });
                        let caption = ui.label(t("Date"));
                        combo.response.labelled_by(caption.id);
                        if self.patients.date != before {
                            let scope = self.patients.scope;
                            self.search(scope);
                        }
                    });
                });
                ui.add_space(8.0);
                self.summary_card(ui);
                ui.add_space(8.0);
                self.patient_list(ui);
            });
    }

    fn summary_card(&mut self, ui: &mut Ui) {
        let patient = self.patients.selected_patient().cloned();
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(84.0, 84.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 40.0, theme::button_hover());
                icons::paint(ui.painter(), rect.shrink(16.0), Icon::Patient, theme::text());
                ui.add_space(12.0);
                egui::Grid::new("summary").num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                    let d = patient.as_ref().map(|p| &p.details);
                    let rows = [
                        (t("Chart No."), patient.as_ref().map(|p| p.id.clone())),
                        (t("Name"), d.map(|d| d.full_name())),
                        (t("Gender"), d.map(|d| d.sex.clone())),
                        (t("Date of Birth"), d.map(|d| d.birthdate.clone())),
                    ];
                    for (label, value) in rows {
                        ui.label(RichText::new(label).color(theme::muted()));
                        ui.label(value.unwrap_or_default());
                        ui.end_row();
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    if icons::button(ui, Some(Icon::Detail), t("Show Detail"), vec2(150.0, 32.0), IconLayout::Inline, patient.is_some(), false).clicked() {
                        self.patients.detail = true;
                    }
                });
            });
        });
    }

    fn patient_list(&mut self, ui: &mut Ui) {
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(ui.available_height());
            section(ui, t("PATIENT SEARCH RESULT"));
            let columns = [0.2, 0.32, 0.22, 0.26];
            super::list_header(ui, &columns.iter().copied().zip([t("Chart No."), t("Name"), t("Date of Birth"), t("Phone")]).collect::<Vec<_>>());
            if self.patients.rows.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new(t("No patients to show. Use New Patient to add one.")).color(theme::muted()));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut clicked = None;
                let mut opened = None;
                for (i, p) in self.patients.rows.iter().enumerate() {
                    let d = &p.details;
                    let name = d.full_name();
                    let label = format!("{} {}", p.id, name);
                    let cells = [(columns[0], p.id.as_str()), (columns[1], name.as_str()), (columns[2], d.birthdate.as_str()), (columns[3], d.phone.as_str())];
                    let selected = self.patients.selected == Some(p.pk);
                    let row = list_row(ui, &label, &cells, 30.0, selected, i % 2 == 1, None, vec2(0.0, 0.0));
                    if row.clicked() {
                        clicked = Some(p.pk);
                    }
                    if row.double_clicked() {
                        opened = Some(p.pk);
                    }
                }
                if let Some(pk) = clicked.or(opened) {
                    self.patients.selected = Some(pk);
                }
                if opened.is_some() {
                    self.open_editor(true);
                }
            });
        });
    }

    pub(super) fn patient_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(editor) = &mut self.patients.editor {
            let editing = editor.pk.is_some();
            let modal = egui::Modal::new(Id::unique("patient-editor")).show(ctx, |ui| {
                ui.set_width(560.0);
                ui.heading(if editing { t("EDIT PATIENT") } else { t("NEW PATIENT") });
                ui.add_space(6.0);
                egui::Grid::new("patient-fields").num_columns(4).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                    for (i, label) in FIELDS.iter().enumerate() {
                        let caption = ui.label(t(label));
                        // Fixed size: a Grid measures columns on its first pass.
                        let enabled = !(editing && i == 0);
                        let field = ui.add_enabled_ui(enabled, |ui| ui.add_sized([180.0, 26.0], egui::TextEdit::singleline(&mut editor.fields[i]))).inner;
                        field.labelled_by(caption.id);
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let cancel = ui.add_sized([90.0, 30.0], egui::Button::new(t("Cancel"))).clicked();
                    let ok = ui.add_sized([90.0, 30.0], egui::Button::new(t("Ok"))).clicked();
                    (ok, cancel)
                })
                .inner
            });
            let (ok, cancel) = modal.inner;
            if ok {
                let saved = self.save_editor();
                self.check(saved);
            } else if cancel || modal.should_close() && self.notice.is_none() {
                self.patients.editor = None;
            }
        }
        if self.patients.detail {
            let patient = self.patients.selected_patient().cloned();
            let modal = egui::Modal::new(Id::unique("patient-detail")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.heading(t("PATIENT DETAIL INFORMATION"));
                ui.add_space(6.0);
                if let Some(p) = &patient {
                    egui::Grid::new("detail").num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                        for (label, value) in FIELDS.iter().zip(values(p)) {
                            ui.label(RichText::new(t(label)).color(theme::muted()));
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                }
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.button(t("Close")).clicked()).inner
            });
            if modal.inner || modal.should_close() || patient.is_none() {
                self.patients.detail = false;
            }
        }
    }
}
