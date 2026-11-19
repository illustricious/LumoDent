//! Colours and widget styling. The dark grey and steel blue palette follows
//! the reference screen layout; every value here is LumoDent's own.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

pub const BACK: Color32 = Color32::from_rgb(0x50, 0x50, 0x50);
pub const PANEL: Color32 = Color32::from_rgb(0x46, 0x46, 0x46);
pub const EDGE: Color32 = Color32::from_rgb(0x62, 0x62, 0x62);
pub const BUTTON: Color32 = Color32::from_rgb(0x4c, 0x5f, 0x78);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x5a, 0x70, 0x8c);
pub const BUTTON_DOWN: Color32 = Color32::from_rgb(0x3e, 0x4e, 0x63);
pub const INPUT: Color32 = Color32::from_rgb(0x53, 0x5e, 0x6a);
pub const ACCENT: Color32 = Color32::from_rgb(0x4e, 0x78, 0x9f);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x8f, 0xb8, 0xe0);
pub const TEXT: Color32 = Color32::from_rgb(0xf0, 0xf0, 0xf0);
pub const MUTED: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xa8);
pub const DISABLED: Color32 = Color32::from_rgb(0x58, 0x5c, 0x62);
pub const CANVAS: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);

pub fn apply(ctx: &egui::Context) {
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
        *v = egui::Visuals::dark();
        v.panel_fill = BACK;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, EDGE);
        v.extreme_bg_color = INPUT;
        v.faint_bg_color = Color32::from_rgb(0x4b, 0x4b, 0x4b);
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.hyperlink_color = ACCENT_LIGHT;
        let radius = CornerRadius::same(3);
        for (w, fill) in [
            (&mut v.widgets.inactive, BUTTON),
            (&mut v.widgets.hovered, BUTTON_HOVER),
            (&mut v.widgets.active, BUTTON_DOWN),
            (&mut v.widgets.open, BUTTON_DOWN),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.corner_radius = radius;
            w.fg_stroke = Stroke::new(1.5, TEXT);
        }
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, EDGE);
        v.widgets.noninteractive.bg_fill = PANEL;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, EDGE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    });
}

/// A bordered panel like the reference application's grouped areas.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, EDGE))
        .corner_radius(CornerRadius::same(2))
        .inner_margin(egui::Margin::same(8))
}
