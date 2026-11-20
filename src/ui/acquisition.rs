//! Acquisition screen: the patient, the X-ray generator settings the
//! reference shows beside the capture area (DC/AC, kV, mA), and the sensor.
//! A sensor is either a real NanoPix-E found on the USB bus (src/usb)
//! or the built-in simulator (src/simulator.rs); both run the same sensor_io
//! protocol on a worker thread that waits for X-rays and reads the frame.

use super::{section, t, theme, App};
use eframe::egui::{self, vec2, Ui};
use lumodent::usb::{self as sensor_usb, Found};
use lumodent::settings::{KV, MA_AC, MA_DC};
use lumodent::usb_sensor::{self, Sensor, Transport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// X-ray trigger threshold above the dark baseline.
const THRESHOLD: i32 = 50;

/// What the sensor reported about itself when it was opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Details {
    pub model: String,
    pub serial: String,
    pub firmware: [String; 2],
    /// USB vendor and product ID (none for the simulator).
    pub usb: Option<(u16, u16)>,
    pub width: u16,
    pub height: u16,
    pub bits: u8,
    pub generation: u8,
    pub hdr_mode: u8,
}

/// The model: NanoPix-E by its product ID (0x44), the others (NanoPix 1, 1.5,
/// 2) by the frame size the sensor reports, like JudgeSensorSize; a size a few
/// border rows off still matches the nearest model.
pub fn model_name(product: u16, width: usize, height: usize, simulated: bool) -> String {
    let gap = |m: &lumodent::sensor::SensorModel| {
        (m.width.abs_diff(width) + m.height.abs_diff(height)).min(m.width.abs_diff(height) + m.height.abs_diff(width))
    };
    let by_size = lumodent::sensor::MODELS.iter().min_by_key(|m| gap(m)).filter(|m| gap(m) < 200);
    let base = match (product, by_size) {
        (0x44, _) => "NanoPix-E".to_string(),
        (_, Some(m)) => m.name.to_string(),
        (p, None) => format!("NanoPix (0x{p:02X})"),
    };
    if simulated { format!("{base} ({})", t("Simulator")) } else { base }
}

pub enum Event {
    Connected(Details),
    Status(&'static str),
    Frame { raw: Vec<u8>, width: usize, height: usize, serial: String },
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Simulator,
    Usb(Found),
}

#[derive(Default)]
pub struct Capture {
    pub devices: Vec<Found>,
    pub searched: bool,
    last_search: Option<std::time::Instant>,
    /// A search running on a worker thread: enumerating USB devices through
    /// SetupAPI can take tens of milliseconds, too long for the UI thread.
    pending: Option<Receiver<Vec<Found>>>,
    pub details: Option<Details>,
    rx: Option<Receiver<Event>>,
    stop: Option<Arc<AtomicBool>>,
    /// The simulator's exposure switch.
    pub exposure: Option<Arc<AtomicBool>>,
    pub connected: Option<String>,
    pub status: String,
    pub captured: usize,
}

impl Capture {
    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    /// Starts looking for sensors bound to the NanoPix driver, off the UI thread.
    fn search(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(sensor_usb::find_sensors());
        });
        self.pending = Some(rx);
        self.last_search = Some(std::time::Instant::now());
    }

    /// Searches every two seconds while nothing is connected and connects to
    /// the first sensor found, as the reference does.
    pub fn auto_connect(&mut self) {
        if self.running() {
            return;
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(devices) => {
                    self.pending = None;
                    self.devices = devices;
                    self.searched = true;
                    if let Some(found) = self.devices.first().cloned() {
                        self.start(Source::Usb(found));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => self.pending = None,
            }
        } else if self.last_search.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            self.search();
        }
    }

    pub fn start(&mut self, source: Source) {
        self.disconnect();
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let transport: Result<Box<dyn Transport + Send>, String> = match &source {
            Source::Simulator => {
                let (sim, exposure) = lumodent::simulator::Simulator::new();
                self.exposure = Some(exposure);
                Ok(Box::new(sim))
            }
            Source::Usb(found) => sensor_usb::open(found),
        };
        let transport = match transport {
            Ok(t) => t,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        self.status = t("Connecting...").into();
        let usb = match &source {
            Source::Simulator => None,
            Source::Usb(f) => Some((f.vendor, f.product)),
        };
        let flag = stop.clone();
        std::thread::spawn(move || {
            let Some(mut sensor) = Sensor::open(transport) else {
                let _ = tx.send(Event::Failed(t("The sensor did not answer.").into()));
                return;
            };
            sensor.set_threshold(THRESHOLD);
            let (w, h) = (sensor.info.width as usize, sensor.info.height as usize);
            let _ = tx.send(Event::Connected(Details {
                model: model_name(sensor.info.product, sensor.info.width as usize, sensor.info.height as usize, usb.is_none()),
                serial: sensor.info.serial.clone(),
                firmware: sensor.firmware.clone(),
                usb,
                width: sensor.info.width,
                height: sensor.info.height,
                bits: sensor.image_bits,
                generation: sensor.generation,
                hdr_mode: sensor.hdr_mode,
            }));
            while !flag.load(Ordering::SeqCst) {
                if sensor.xray_detected() {
                    let _ = tx.send(Event::Status("Reading image..."));
                    match sensor.load_raw_image() {
                        Some(raw) => {
                            let _ = tx.send(Event::Frame { raw, width: w, height: h, serial: sensor.info.serial.clone() });
                        }
                        None => {
                            let _ = tx.send(Event::Status("Image acquisition failed"));
                        }
                    }
                } else {
                    let _ = tx.send(Event::Status("Ready for exposure"));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            sensor.close();
        });
        self.rx = Some(rx);
        self.stop = Some(stop);
    }

    pub fn disconnect(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.rx = None;
        self.exposure = None;
        self.connected = None;
        self.details = None;
        self.status.clear();
    }

    fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(e) = rx.try_recv() {
                out.push(e);
            }
        }
        out
    }
}

impl App {
    /// Handles what the sensor thread reported: a captured frame is saved
    /// for the acquisition patient.
    fn capture_events(&mut self) {
        for event in self.capture.poll() {
            match event {
                Event::Connected(d) => {
                    self.capture.connected = Some(format!("{}  SN {}", d.model, d.serial));
                    self.capture.details = Some(d);
                }
                Event::Status(s) => self.capture.status = t(s).into(),
                Event::Failed(e) => {
                    self.capture.disconnect();
                    self.capture.status = e;
                }
                Event::Frame { raw, width, height, serial } => {
                    let dir = self.ws.data.join("calibration");
                    let cal = dir.is_dir().then_some((dir.as_path(), serial.as_str()));
                    let image = usb_sensor::frame_to_image(&raw, width, height, cal);
                    let Some(patient) = self.acquisition_patient.clone() else { continue };
                    let saved = self.ws.save_capture(&patient, &image);
                    if self.check(saved).is_some() {
                        self.capture.captured += 1;
                        self.capture.status = t("Image saved. Open the Viewer to see it.").into();
                    }
                }
            }
        }
    }

    pub(super) fn acquisition_screen(&mut self, ui: &mut Ui) {
        self.capture_events();
        if self.capture.running() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
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
                    ui.add_space(16.0);
                    section(ui, t("SENSOR"));
                    self.sensor_panel(ui);
                });
            });
    }

    fn sensor_panel(&mut self, ui: &mut Ui) {
        if let Some(connected) = self.capture.connected.clone() {
            let green = egui::Color32::from_rgb(0x4c, 0xc3, 0x6b);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 5.0, green);
                ui.label(egui::RichText::new(t("Sensor detected")).color(green).strong());
            });
            ui.label(egui::RichText::new(connected).size(18.0).strong());
            if let Some(d) = self.capture.details.clone() {
                egui::Grid::new("sensor-details").num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                    let mut row = |k: &'static str, v: String| {
                        ui.label(egui::RichText::new(t(k)).color(theme::muted()));
                        ui.label(v);
                        ui.end_row();
                    };
                    row("Device type", d.model.clone());
                    row("Serial number", d.serial.clone());
                    row("Firmware", d.firmware.iter().filter(|f| !f.is_empty()).cloned().collect::<Vec<_>>().join(" / "));
                    row("USB ID", d.usb.map(|(v, p)| format!("{v:04X}:{p:04X}")).unwrap_or_else(|| "-".into()));
                    row("Resolution", format!("{} × {}", d.width, d.height));
                    row("Bit depth", if d.bits == 0 { "-".into() } else { format!("{} bit", d.bits) });
                    row("HDR", if d.hdr_mode == 1 { t("On").into() } else { t("Off").into() });
                });
                ui.add_space(6.0);
            }
            ui.label(egui::RichText::new(&self.capture.status).size(18.0).color(theme::accent()));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if let Some(exposure) = self.capture.exposure.clone() {
                    let busy = exposure.load(Ordering::SeqCst);
                    if ui.add_enabled(!busy, egui::Button::new(t("Simulate Exposure")).min_size(vec2(160.0, 32.0))).clicked() {
                        exposure.store(true, Ordering::SeqCst);
                    }
                }
                if ui.add_sized([120.0, 32.0], egui::Button::new(t("Disconnect"))).clicked() {
                    self.capture.disconnect();
                }
            });
            if self.capture.captured > 0 {
                ui.label(format!("{}: {}", t("Images captured"), self.capture.captured));
            }
            return;
        }
        self.capture.auto_connect();
        // Wake twice a second so the background search result and the next
        // search happen without mouse movement (this screen only).
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        if self.capture.running() {
            ui.label(egui::RichText::new(&self.capture.status).size(18.0));
            return;
        }
        if self.capture.devices.is_empty() {
            ui.label(egui::RichText::new(t("Sensor not connected")).size(18.0).color(theme::muted()));
            ui.label(t("Searching for a sensor. It connects by itself once plugged in."));
        }
        let mut start = None;
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_sized([160.0, 32.0], egui::Button::new(t("Use Simulator"))).clicked() {
                start = Some(Source::Simulator);
            }
        });
        if !self.capture.status.is_empty() {
            ui.label(&self.capture.status);
        }
        if let Some(source) = start {
            self.capture.start(source);
        }
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
