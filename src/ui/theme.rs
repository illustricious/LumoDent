//! Colours and widget styling, in a dark and a light palette. Every value
//! here is LumoDent's own.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};
use std::sync::atomic::{AtomicBool, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);

struct Palette {
    back: Color32,
    panel: Color32,
    edge: Color32,
    button: Color32,
    button_hover: Color32,
    button_down: Color32,
    input: Color32,
    accent: Color32,
    accent_light: Color32,
    text: Color32,
    muted: Color32,
    disabled: Color32,
    canvas: Color32,
    faint: Color32,
}

const fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

const DARK: Palette = Palette {
    back: rgb(0x16_18_1b),
    panel: rgb(0x1f_22_26),
    edge: rgb(0x34_39_40),
    button: rgb(0x2b_3f_57),
    button_hover: rgb(0x36_4f_6d),
    button_down: rgb(0x22_33_47),
    input: rgb(0x11_13_16),
    accent: rgb(0x3d_7b_c4),
    accent_light: rgb(0x8f_b8_e0),
    text: rgb(0xec_ee_f1),
    muted: rgb(0x9a_a1_ab),
    disabled: rgb(0x3a_3f_46),
    canvas: rgb(0x00_00_00),
    faint: rgb(0x1b_1e_21),
};

const LIGHT_PALETTE: Palette = Palette {
    back: rgb(0xee_f1_f4),
    panel: rgb(0xff_ff_ff),
    edge: rgb(0xc9_d0_d8),
    button: rgb(0xdc_e6_f2),
    button_hover: rgb(0xc8_d9_ec),
    button_down: rgb(0xb4_cb_e5),
    input: rgb(0xf7_f9_fb),
    accent: rgb(0xae_cb_ee),
    accent_light: rgb(0x2f_6f_b8),
    text: rgb(0x1b_1f_24),
    muted: rgb(0x5d_66_70),
    disabled: rgb(0xb0_b7_bf),
    canvas: rgb(0x0b_0c_0d),
    faint: rgb(0xf3_f5_f7),
};

fn p() -> &'static Palette {
    if is_light() { &LIGHT_PALETTE } else { &DARK }
}

pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

pub fn back() -> Color32 { p().back }
pub fn panel() -> Color32 { p().panel }
pub fn edge() -> Color32 { p().edge }
pub fn button() -> Color32 { p().button }
pub fn button_hover() -> Color32 { p().button_hover }
pub fn button_down() -> Color32 { p().button_down }
pub fn accent() -> Color32 { p().accent }
pub fn accent_light() -> Color32 { p().accent_light }
pub fn text() -> Color32 { p().text }
pub fn muted() -> Color32 { p().muted }
pub fn disabled() -> Color32 { p().disabled }
pub fn canvas() -> Color32 { p().canvas }
pub fn stripe() -> Color32 { p().faint }

/// Switches the palette and restyles every widget.
pub fn set_light(ctx: &egui::Context, light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
    apply(ctx);
}

pub fn apply(ctx: &egui::Context) {
    let c = p();
    let light = is_light();
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(17.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        let v = &mut style.visuals;
        *v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
        v.panel_fill = c.back;
        v.window_fill = c.panel;
        v.window_stroke = Stroke::new(1.0, c.edge);
        v.extreme_bg_color = c.input;
        v.faint_bg_color = c.faint;
        v.override_text_color = Some(c.text);
        v.selection.bg_fill = c.accent;
        v.selection.stroke = Stroke::new(1.0, c.text);
        v.hyperlink_color = c.accent_light;
        let radius = CornerRadius::same(4);
        for (w, fill) in [
            (&mut v.widgets.inactive, c.button),
            (&mut v.widgets.hovered, c.button_hover),
            (&mut v.widgets.active, c.button_down),
            (&mut v.widgets.open, c.button_down),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = radius;
            w.fg_stroke = Stroke::new(1.5, c.text);
        }
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, c.edge);
        v.widgets.noninteractive.bg_fill = c.panel;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.edge);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    });
}

/// A bordered panel for grouped areas.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(panel())
        .stroke(Stroke::new(1.0, edge()))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(egui::Margin::same(8))
}
