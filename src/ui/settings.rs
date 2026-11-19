//! The Settings dialog: five tabs like the reference (General, Patient,
//! Acquisition, Viewer, Report), edited on a copy and applied with OK.

use super::{i18n, t, App};
use eframe::egui::{self, vec2, Color32, Id, Layout, Ui};
use lumodent::reports::Clinic;
use lumodent::settings::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    General,
    Patient,
    Acquisition,
    Viewer,
    Report,
}

impl Tab {
    pub(super) const ALL: [Tab; 5] = [Tab::General, Tab::Patient, Tab::Acquisition, Tab::Viewer, Tab::Report];
    fn label(self) -> &'static str {
        match self {
            Tab::General => t("General"),
            Tab::Patient => t("Patient"),
            Tab::Acquisition => t("Acquisition"),
            Tab::Viewer => t("Viewer"),
            Tab::Report => t("Report"),
        }
    }
}

pub struct Dialog {
    pub(super) tab: Tab,
    s: Settings,
    clinic: Clinic,
    indonesian: bool,
    light: bool,
}

/// Where a row leaves its caption's id for the widgets in it.
const ROW_LABEL: &str = "settings-row-label";

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let caption = ui.label(label);
    ui.data_mut(|d| d.insert_temp(Id::unique(ROW_LABEL), caption.id));
    add(ui);
    ui.end_row();
}

/// A captioned text field; the caption names the field for screen readers.
fn text_row(ui: &mut Ui, label: &str, value: &mut String, max: usize, width: f32) {
    let caption = ui.label(label);
    ui.add(egui::TextEdit::singleline(value).char_limit(max).desired_width(width)).labelled_by(caption.id);
    ui.end_row();
}

fn section(ui: &mut Ui, title: &str, id: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title).strong());
    super::theme::panel_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new(id).num_columns(2).min_col_width(170.0).spacing(vec2(12.0, 8.0)).show(ui, body);
    });
}

fn combo<T: std::fmt::Display>(ui: &mut Ui, id: &str, index: &mut usize, items: &[T]) {
    let caption: Option<Id> = ui.data(|d| d.get_temp(Id::unique(ROW_LABEL)));
    let response = egui::ComboBox::from_id_salt(id).width(160.0).selected_text(items[(*index).min(items.len() - 1)].to_string()).show_ui(ui, |ui| {
        for (i, item) in items.iter().enumerate() {
            ui.selectable_value(index, i, item.to_string());
        }
    });
    if let Some(caption) = caption {
        response.response.labelled_by(caption);
    }
}

fn on_off(ui: &mut Ui, value: &mut bool) {
    ui.horizontal(|ui| {
        ui.radio_value(value, true, t("On"));
        ui.radio_value(value, false, t("Off"));
    });
}

fn color(ui: &mut Ui, c: &mut Rgba) {
    let mut c32 = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    if ui.color_edit_button_srgba(&mut c32).changed() {
        *c = c32.to_srgba_unmultiplied();
    }
}

fn thickness(ui: &mut Ui, id: &str, value: &mut u32) {
    let mut i = THICKNESS.iter().position(|v| v == value).unwrap_or(0);
    let labels: Vec<String> = THICKNESS.iter().map(|v| format!("{v}px")).collect();
    combo(ui, id, &mut i, &labels);
    *value = THICKNESS[i];
}

fn stroke(ui: &mut Ui, id: &str, label: &str, s: &mut Stroke) {
    row(ui, label, |ui| {
        ui.horizontal(|ui| {
            color(ui, &mut s.color);
            thickness(ui, id, &mut s.thickness);
        });
    });
}

impl Dialog {
    fn general(&mut self, ui: &mut Ui) {
        section(ui, t("Language"), "set-lang", |ui| {
            row(ui, t("Language"), |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.indonesian, false, "English");
                    ui.radio_value(&mut self.indonesian, true, "Bahasa Indonesia");
                });
            });
        });
        section(ui, t("User Style"), "set-style", |ui| {
            row(ui, t("Icon Size"), |ui| {
                ui.horizontal(|ui| {
                    for (size, label) in IconSize::ALL.into_iter().zip([t("Small"), t("Medium"), t("Big")]) {
                        ui.radio_value(&mut self.s.icon_size, size, label);
                    }
                });
            });
            row(ui, t("Theme"), |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.light, false, t("Dark"));
                    ui.radio_value(&mut self.light, true, t("Light"));
                });
            });
        });
        section(ui, t("Clinic"), "set-clinic", |ui| {
            text_row(ui, t("Clinic Name"), &mut self.clinic.name, 127, 280.0);
            text_row(ui, t("Clinic Address"), &mut self.clinic.address, 319, 280.0);
            text_row(ui, t("Clinic Phone"), &mut self.clinic.phone, 15, 160.0);
        });
    }

    fn patient(&mut self, ui: &mut Ui) {
        section(ui, t("Automatic Chart Number"), "set-chart", |ui| {
            row(ui, t("Chart Number"), |ui| on_off(ui, &mut self.s.auto_chart_number));
        });
        section(ui, t("Name Option"), "set-name", |ui| {
            row(ui, t("Name Order"), |ui| {
                let mut i = self.s.last_name_first as usize;
                combo(ui, "name-order", &mut i, &[t("First Last"), t("Last First")]);
                self.s.last_name_first = i == 1;
            });
        });
        section(ui, t("Quick Search"), "set-quick", |ui| {
            for (label, value, id) in [
                (t("Recently Acquired"), &mut self.s.recent_acquired_days, "recent-acq"),
                (t("Recently Printed"), &mut self.s.recent_printed_days, "recent-print"),
            ] {
                row(ui, label, |ui| {
                    ui.horizontal(|ui| {
                        for d in RECENT_DAYS {
                            ui.radio_value(value, d, format!("{d} {}", t("days")));
                        }
                    });
                });
                let _ = id;
            }
        });
    }

    fn acquisition(&mut self, ui: &mut Ui, data: &std::path::Path) {
        section(ui, t("Trigger Setting"), "set-trigger", |ui| {
            row(ui, t("Generator Type"), |ui| {
                let mut ac = self.s.generator_ac;
                ui.horizontal(|ui| {
                    ui.radio_value(&mut ac, false, "DC");
                    ui.radio_value(&mut ac, true, "AC");
                });
                if ac != self.s.generator_ac {
                    self.s.set_generator_ac(ac);
                }
            });
            row(ui, t("Generator kV"), |ui| combo(ui, "kv", &mut self.s.kv, &KV));
            row(ui, t("Generator mA"), |ui| {
                let choices = self.s.ma_choices();
                combo(ui, "ma", &mut self.s.ma, choices);
            });
        });
        section(ui, t("Image Calibration"), "set-cali", |ui| {
            let caption = ui.label(t("Millimetres per Pixel"));
                ui.add(egui::DragValue::new(&mut self.s.calibration_mm).range(0.005..=1.0).speed(0.001).max_decimals(3)).labelled_by(caption.id);
                ui.end_row();
        });
        section(ui, t("Patient Data Path"), "set-path", |ui| {
            row(ui, t("Folder"), |ui| {
                ui.add(egui::Label::new(data.display().to_string()).truncate());
            });
        });
        ui.label(egui::RichText::new(t("Trigger and generator values are stored for acquisition, which needs the sensor.")).small().weak());
    }

    fn viewer(&mut self, ui: &mut Ui) {
        section(ui, t("Display"), "set-display", |ui| {
            row(ui, t("Full Screen"), |ui| on_off(ui, &mut self.s.fullscreen));
            row(ui, t("Image Layout"), |ui| combo(ui, "layout", &mut self.s.layout, &LAYOUTS));
        });
        section(ui, t("Magnifier"), "set-mag", |ui| {
            row(ui, t("Size"), |ui| {
                let labels: Vec<String> = MAGNIFIER_SIZES.iter().map(|s| format!("{s}x{s}")).collect();
                combo(ui, "mag-size", &mut self.s.magnifier_size, &labels);
            });
            row(ui, t("Ratio"), |ui| combo(ui, "mag-ratio", &mut self.s.magnifier_ratio, &MAGNIFIER_RATIOS));
            row(ui, t("Capture Border"), |ui| color(ui, &mut self.s.capture_border));
        });
        section(ui, t("Grid"), "set-grid", |ui| {
            row(ui, t("Spacing"), |ui| {
                let labels: Vec<String> = GRID_MM.iter().map(|v| format!("{v}mm")).collect();
                combo(ui, "grid-mm", &mut self.s.grid_mm, &labels);
            });
            stroke(ui, "grid-t", t("Line"), &mut self.s.grid);
            row(ui, t("Opacity"), |ui| {
                let labels: Vec<String> = GRID_OPACITY.iter().map(|v| format!("{v}%")).collect();
                combo(ui, "grid-op", &mut self.s.grid_opacity, &labels);
            });
        });
        section(ui, t("Measurement"), "set-measure", |ui| {
            stroke(ui, "len-t", t("Length"), &mut self.s.length);
            row(ui, t("Show Length Unit"), |ui| on_off(ui, &mut self.s.length_unit));
            stroke(ui, "ang-t", t("Angle"), &mut self.s.angle);
            row(ui, t("Show Angle Unit"), |ui| on_off(ui, &mut self.s.angle_unit));
        });
        section(ui, t("Drawing"), "set-draw", |ui| {
            stroke(ui, "draw-t", t("Free Draw"), &mut self.s.draw);
            stroke(ui, "line-t", t("Line"), &mut self.s.line);
            stroke(ui, "rect-t", t("Rectangle"), &mut self.s.rectangle);
            stroke(ui, "ell-t", t("Ellipse"), &mut self.s.ellipse);
        });
        section(ui, t("Memo Style"), "set-memo", |ui| {
            row(ui, t("Outer Color"), |ui| color(ui, &mut self.s.memo_outer));
            row(ui, t("Text Color"), |ui| color(ui, &mut self.s.memo_text));
            for (i, preset) in self.s.memo_presets.iter_mut().enumerate() {
                text_row(ui, &format!("{} {}", t("Preset"), i + 1), preset, 127, 280.0);
            }
        });
    }

    fn report(&mut self, ui: &mut Ui) {
        section(ui, t("Border Style"), "set-border", |ui| {
            stroke(ui, "border-t", t("Border Line"), &mut self.s.border);
        });
        section(ui, t("Text Box"), "set-text", |ui| {
            row(ui, t("Font Size"), |ui| {
                let labels: Vec<String> = FONT_SIZES.iter().map(|v| format!("{v}")).collect();
                combo(ui, "font-size", &mut self.s.font_size, &labels);
            });
            row(ui, t("Text Color"), |ui| color(ui, &mut self.s.text_color));
        });
        ui.label(egui::RichText::new(t("Paper size, orientation, header and footer are set per report on the Report screen.")).small().weak());
    }
}

impl App {
    pub(super) fn open_settings(&mut self) {
        self.settings_dialog = Some(Dialog {
            tab: Tab::General,
            s: self.settings.clone(),
            clinic: self.report.clinic.clone(),
            indonesian: i18n::is_indonesian(),
            light: super::theme::is_light(),
        });
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.settings_dialog else { return };
        let data = self.ws.data.clone();
        let modal = egui::Modal::new(Id::unique("settings")).show(ctx, |ui| {
            let size = ctx.content_rect().size();
            ui.set_width((size.x * 0.7).clamp(560.0, 900.0));
            ui.heading(t("Settings"));
            ui.add_space(6.0);
            let height = (size.y * 0.7).clamp(320.0, 640.0);
            // A fixed-height body: inside a modal the available height is
            // unbounded, so the separator and scroll area would grow without it.
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height), Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_height(height);
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    for tab in Tab::ALL {
                        if ui.add_sized(vec2(150.0, 34.0), egui::Button::selectable(dialog.tab == tab, tab.label())).clicked() {
                            dialog.tab = tab;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_width(ui.available_width() - 12.0);
                        match dialog.tab {
                            Tab::General => dialog.general(ui),
                            Tab::Patient => dialog.patient(ui),
                            Tab::Acquisition => dialog.acquisition(ui, &data),
                            Tab::Viewer => dialog.viewer(ui),
                            Tab::Report => dialog.report(ui),
                        }
                    });
                });
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                let cancel = ui.button(t("Cancel")).clicked();
                let ok = ui.button(t("OK")).clicked();
                (ok, cancel)
            })
            .inner
        });
        let (ok, cancel) = modal.inner;
        if ok {
            let dialog = self.settings_dialog.take().unwrap();
            if dialog.indonesian != i18n::is_indonesian() {
                self.set_language(dialog.indonesian);
            }
            if dialog.light != super::theme::is_light() {
                self.set_theme(ctx, dialog.light);
            }
            if dialog.s.fullscreen != self.settings.fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(dialog.s.fullscreen));
            }
            self.report.clinic = dialog.clinic;
            let saved = self.ws.save_clinic(&self.report.clinic).and_then(|_| dialog.s.save(&self.ws.data.join("settings.txt")));
            self.settings = dialog.s;
            ctx.set_zoom_factor(self.settings.icon_size.zoom());
            self.check(saved);
        } else if cancel || modal.should_close() {
            self.settings_dialog = None;
        }
    }
}
