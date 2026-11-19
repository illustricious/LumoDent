//! Acquisition screen: the patient, the X-ray generator settings the
//! reference shows beside the capture area (DC/AC, kV, mA) and the sensor
//! state. Capturing needs the detector, which LumoDent does not drive yet.

use super::{section, t, theme, App};
use eframe::egui::{self, vec2, Ui};
use lumodent::settings::{KV, MA_AC, MA_DC};

impl App {
    pub(super) fn acquisition_screen(&mut self, ui: &mut Ui) {
        egui::Panel::left("acq-left")
            .exact_size(self.left_width.max(260.0))
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(ui, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(ui.available_height());
                    section(ui, t("GENERATOR"));
                    let mut changed = false;
                    ui.label(t("Generator Type"));
                    ui.horizontal(|ui| {
                        let mut ac = self.settings.generator_ac;
                        ui.radio_value(&mut ac, false, "DC");
                        ui.radio_value(&mut ac, true, "AC");
                        if ac != self.settings.generator_ac {
                            self.settings.set_generator_ac(ac);
                            changed = true;
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(t("Generator kV"));
                    changed |= choice_grid(ui, "kv", &mut self.settings.kv, &KV);
                    ui.add_space(8.0);
                    ui.label(t("Generator mA"));
                    let choices: &[&str] = if self.settings.generator_ac { &MA_AC } else { &MA_DC };
                    changed |= choice_grid(ui, "ma", &mut self.settings.ma, choices);
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(format!(
                        "{}: {} kV, {} mA ({})",
                        t("Selected"),
                        KV[self.settings.kv],
                        choices[self.settings.ma.min(choices.len() - 1)],
                        if self.settings.generator_ac { "AC" } else { "DC" }
                    ))
                    .strong());
                    if changed {
                        let saved = self.settings.save(&self.ws.data.join("settings.txt"));
                        self.check(saved);
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(8)))
            .show(ui, |ui| {
                theme::panel_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(ui.available_height());
                    let name = self.context_patient().map(|p| format!("{}  {} {}", p.id, p.details.first_name, p.details.last_name)).unwrap_or_default();
                    section(ui, t("ACQUISITION"));
                    ui.label(egui::RichText::new(name).size(18.0));
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(t("Sensor not connected")).size(20.0).color(theme::muted()));
                        ui.add_space(6.0);
                        ui.label(t("Capturing needs the sensor driver (sensor_io.dll for NanoPix-E, IOSensorAcquisition.dll for NanoPix 1/1.5/2), which LumoDent does not replace yet. Images can be imported on the Viewer screen."));
                    });
                });
            });
    }
}

/// Toggle buttons for one choice out of a list, wrapped onto rows.
fn choice_grid(ui: &mut Ui, id: &str, index: &mut usize, items: &[&str]) -> bool {
    let mut changed = false;
    ui.push_id(id, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (i, item) in items.iter().enumerate() {
                let on = *index == i;
                if ui.add_sized(vec2(64.0, 30.0), egui::Button::new(*item).selected(on)).clicked() && !on {
                    *index = i;
                    changed = true;
                }
            }
        });
    });
    changed
}
