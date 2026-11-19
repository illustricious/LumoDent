//! Icons drawn as vector shapes, plus the buttons that carry them. Nothing
//! here comes from the reference package's artwork.

use super::theme;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, Role, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Patient,
    Acquisition,
    Viewer,
    Report,
    AddPatient,
    Edit,
    Delete,
    Clear,
    Search,
    Detail,
    Clock,
    Print,
    ZoomIn,
    ZoomOut,
    Rotate,
    RotateLeft,
    Reset,
    ExportImage,
    Measure,
    Filter,
    FlipH,
    FlipV,
    Invert,
    Fit,
    Send,
    Import,
    Save,
    Pdf,
    NewDoc,
}

fn arc(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let steps = ((to - from).abs() / TAU * 48.0).ceil().max(4.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = from + (to - from) * i as f32 / steps as f32;
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

fn arrow_head(p: &Painter, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
    let d = dir.normalized();
    let n = vec2(-d.y, d.x);
    p.add(Shape::convex_polygon(
        vec![tip, tip - d * size + n * size * 0.6, tip - d * size - n * size * 0.6],
        color,
        Stroke::NONE,
    ));
}

fn magnifier(p: &Painter, r: Rect, s: Stroke) -> (Pos2, f32) {
    let c = r.min + r.size() * 0.42;
    let rad = r.width() * 0.27;
    p.circle_stroke(c, rad, s);
    let dir = vec2(1.0, 1.0).normalized();
    p.line_segment([c + dir * rad, r.max - r.size() * 0.1], Stroke::new(s.width * 1.6, s.color));
    (c, rad)
}

fn person(p: &Painter, r: Rect, s: Stroke) {
    let w = r.width();
    p.circle_stroke(pos2(r.center().x, r.top() + w * 0.32), w * 0.18, s);
    let base = pos2(r.center().x, r.bottom() - w * 0.08);
    p.add(Shape::line(arc(base, w * 0.32, PI, TAU), s));
}

fn document(p: &Painter, r: Rect, s: Stroke) -> Rect {
    let page = Rect::from_center_size(r.center(), vec2(r.width() * 0.62, r.height() * 0.8));
    let fold = page.width() * 0.3;
    p.add(Shape::line(
        vec![
            page.left_top(),
            pos2(page.right() - fold, page.top()),
            pos2(page.right(), page.top() + fold),
            page.right_bottom(),
            page.left_bottom(),
            page.left_top(),
        ],
        s,
    ));
    p.add(Shape::line(
        vec![pos2(page.right() - fold, page.top()), pos2(page.right() - fold, page.top() + fold), pos2(page.right(), page.top() + fold)],
        s,
    ));
    page
}

/// Paints `icon` inside `r` with strokes scaled to its size.
pub fn paint(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let w = r.width().min(r.height());
    let r = Rect::from_center_size(r.center(), vec2(w, w));
    let s = Stroke::new((w * 0.075).max(1.2), color);
    let c = r.center();
    let at = |x: f32, y: f32| pos2(r.left() + x * w, r.top() + y * w);
    match icon {
        Icon::Patient => person(p, r, s),
        Icon::AddPatient => {
            person(p, Rect::from_min_size(r.min, vec2(w * 0.8, w)), s);
            p.line_segment([at(0.78, 0.3), at(0.78, 0.62)], s);
            p.line_segment([at(0.62, 0.46), at(0.94, 0.46)], s);
        }
        Icon::Acquisition => {
            // Intraoral sensor with its cable.
            let body = Rect::from_min_max(at(0.18, 0.2), at(0.68, 0.62));
            p.rect_stroke(body, CornerRadius::same((w * 0.08) as u8), s, StrokeKind::Middle);
            p.rect_filled(body.shrink(w * 0.12), 1.0, color.gamma_multiply(0.5));
            p.add(Shape::line(arc(at(0.68, 0.78), w * 0.16, -PI / 2.0, PI / 2.0), s));
            p.line_segment([at(0.68, 0.94), at(0.9, 0.94)], s);
        }
        Icon::Viewer => {
            let frame = Rect::from_min_max(at(0.12, 0.2), at(0.88, 0.8));
            p.rect_stroke(frame, 2.0, s, StrokeKind::Middle);
            p.add(Shape::line(vec![at(0.18, 0.72), at(0.4, 0.46), at(0.55, 0.6), at(0.66, 0.5), at(0.82, 0.72)], s));
            p.circle_filled(at(0.68, 0.34), w * 0.06, color);
        }
        Icon::Report => {
            let page = document(p, r, s);
            for (i, f) in [0.42, 0.56, 0.7].into_iter().enumerate() {
                let right = if i == 2 { 0.55 } else { 0.8 };
                p.line_segment([pos2(page.left() + page.width() * 0.2, page.top() + page.height() * f), pos2(page.left() + page.width() * right, page.top() + page.height() * f)], s);
            }
        }
        Icon::NewDoc => {
            let page = document(p, r.shrink(w * 0.04), s);
            let m = page.center() + vec2(0.0, w * 0.06);
            p.line_segment([m - vec2(w * 0.14, 0.0), m + vec2(w * 0.14, 0.0)], s);
            p.line_segment([m - vec2(0.0, w * 0.14), m + vec2(0.0, w * 0.14)], s);
        }
        Icon::Edit => {
            let (a, b) = (at(0.22, 0.78), at(0.78, 0.22));
            let n = vec2(1.0, 1.0).normalized() * w * 0.08;
            p.add(Shape::line(vec![a + n, b + n, b - n, a - n, a + n], s));
            p.line_segment([at(0.16, 0.84), a], s);
            p.line_segment([at(0.62, 0.3) + n, at(0.62, 0.3) - n], s);
        }
        Icon::Delete => {
            p.line_segment([at(0.2, 0.28), at(0.8, 0.28)], s);
            p.line_segment([at(0.4, 0.28), at(0.42, 0.16)], s);
            p.line_segment([at(0.42, 0.16), at(0.58, 0.16)], s);
            p.line_segment([at(0.58, 0.16), at(0.6, 0.28)], s);
            p.add(Shape::line(vec![at(0.27, 0.28), at(0.32, 0.86), at(0.68, 0.86), at(0.73, 0.28)], s));
            for x in [0.43, 0.57] {
                p.line_segment([at(x, 0.4), at(x, 0.74)], s);
            }
        }
        Icon::Clear => {
            for y in [0.25, 0.45, 0.65] {
                p.line_segment([at(0.12, y), at(0.5, y)], s);
            }
            p.line_segment([at(0.6, 0.55), at(0.88, 0.83)], s);
            p.line_segment([at(0.88, 0.55), at(0.6, 0.83)], s);
        }
        Icon::Search => {
            magnifier(p, r, s);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            let (m, rad) = magnifier(p, r, s);
            p.line_segment([m - vec2(rad * 0.55, 0.0), m + vec2(rad * 0.55, 0.0)], s);
            if icon == Icon::ZoomIn {
                p.line_segment([m - vec2(0.0, rad * 0.55), m + vec2(0.0, rad * 0.55)], s);
            }
        }
        Icon::Detail => {
            p.circle_stroke(c, w * 0.38, s);
            p.circle_filled(at(0.5, 0.3), w * 0.055, color);
            p.line_segment([at(0.5, 0.44), at(0.5, 0.72)], s);
        }
        Icon::Clock => {
            p.circle_stroke(c, w * 0.38, s);
            p.line_segment([c, at(0.5, 0.25)], s);
            p.line_segment([c, at(0.68, 0.6)], s);
        }
        Icon::Print => {
            p.rect_stroke(Rect::from_min_max(at(0.28, 0.12), at(0.72, 0.38)), 1.0, s, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(0.12, 0.38), at(0.88, 0.7)), 2.0, s, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(0.28, 0.58), at(0.72, 0.88)), 1.0, s, StrokeKind::Middle);
            p.circle_filled(at(0.78, 0.48), w * 0.035, color);
        }
        Icon::Rotate | Icon::RotateLeft => {
            // The left arrow is the right one mirrored about the centre.
            let m = if icon == Icon::RotateLeft { -1.0 } else { 1.0 };
            let pts: Vec<Pos2> = arc(c, w * 0.32, -PI * 0.15, PI * 1.45).into_iter().map(|q| pos2(c.x + (q.x - c.x) * m, q.y)).collect();
            let end = *pts.last().unwrap();
            p.add(Shape::line(pts, s));
            arrow_head(p, end + vec2(w * 0.12 * m, w * 0.02), vec2(m, 0.25), w * 0.18, color);
        }
        Icon::Reset => {
            let pts = arc(c, w * 0.3, PI * 0.25, PI * 1.85);
            let start = pts[0];
            p.add(Shape::line(pts, s));
            arrow_head(p, start + vec2(w * 0.02, -w * 0.1), vec2(0.3, -1.0), w * 0.18, color);
            p.circle_filled(c, w * 0.06, color);
        }
        Icon::Filter => {
            for (row, knob) in [(0.25, 0.68), (0.5, 0.32), (0.75, 0.56)] {
                let y = at(0.0, row).y;
                p.line_segment([pos2(at(0.12, 0.0).x, y), pos2(at(0.88, 0.0).x, y)], s);
                p.circle_filled(at(knob, row), w * 0.09, color);
            }
        }
        Icon::Measure => {
            let a = at(0.12, 0.7);
            let dir = vec2(1.0, -0.6).normalized();
            let n = vec2(-dir.y, dir.x) * w * 0.16;
            let len = w * 0.88;
            let b = a + dir * len;
            p.add(Shape::closed_line(vec![a, b, b + n, a + n], s));
            for k in 1..6 {
                let q = a + dir * (len * k as f32 / 6.0);
                let depth = if k % 2 == 0 { 0.55 } else { 0.35 };
                p.line_segment([q, q + n * depth], s);
            }
        }
        Icon::ExportImage => {
            let frame = Rect::from_min_max(at(0.12, 0.2), at(0.72, 0.8));
            p.rect_stroke(frame, 1.0, s, StrokeKind::Middle);
            p.add(Shape::line(vec![at(0.16, 0.72), at(0.34, 0.5), at(0.46, 0.62), at(0.58, 0.44), at(0.68, 0.72)], s));
            p.line_segment([at(0.62, 0.5), at(0.9, 0.5)], s);
            arrow_head(p, at(0.94, 0.5), vec2(1.0, 0.0), w * 0.16, color);
        }
        Icon::FlipH | Icon::FlipV => {
            let (a, b, axis): (Vec<Pos2>, Vec<Pos2>, [Pos2; 2]) = if icon == Icon::FlipH {
                (
                    vec![at(0.42, 0.22), at(0.42, 0.78), at(0.12, 0.78)],
                    vec![at(0.58, 0.22), at(0.58, 0.78), at(0.88, 0.78)],
                    [at(0.5, 0.1), at(0.5, 0.9)],
                )
            } else {
                (
                    vec![at(0.22, 0.42), at(0.78, 0.42), at(0.78, 0.12)],
                    vec![at(0.22, 0.58), at(0.78, 0.58), at(0.78, 0.88)],
                    [at(0.1, 0.5), at(0.9, 0.5)],
                )
            };
            p.add(Shape::convex_polygon(a, color, Stroke::NONE));
            p.add(Shape::closed_line(b, s));
            p.add(Shape::dashed_line(&axis, Stroke::new(s.width * 0.7, color), w * 0.08, w * 0.06));
        }
        Icon::Invert => {
            p.circle_stroke(c, w * 0.36, s);
            let mut half = arc(c, w * 0.36, -PI / 2.0, PI / 2.0);
            half.push(c);
            p.add(Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Fit => {
            let k = w * 0.22;
            for (corner, dx, dy) in [
                (at(0.14, 0.14), 1.0, 1.0),
                (at(0.86, 0.14), -1.0, 1.0),
                (at(0.14, 0.86), 1.0, -1.0),
                (at(0.86, 0.86), -1.0, -1.0),
            ] {
                p.add(Shape::line(vec![corner + vec2(dx * k, 0.0), corner, corner + vec2(0.0, dy * k)], s));
            }
            p.rect_stroke(Rect::from_center_size(c, vec2(w * 0.3, w * 0.3)), 1.0, s, StrokeKind::Middle);
        }
        Icon::Send => {
            let page = document(p, Rect::from_min_size(at(0.3, 0.0), vec2(w * 0.7, w)), s);
            let y = page.center().y;
            p.line_segment([at(0.04, 0.0) + vec2(0.0, y - r.top()), pos2(page.left() + w * 0.2, y)], s);
            arrow_head(p, pos2(page.left() + w * 0.26, y), vec2(1.0, 0.0), w * 0.16, color);
        }
        Icon::Import => {
            p.add(Shape::line(vec![at(0.14, 0.6), at(0.14, 0.86), at(0.86, 0.86), at(0.86, 0.6)], s));
            p.line_segment([at(0.5, 0.12), at(0.5, 0.56)], s);
            arrow_head(p, at(0.5, 0.7), vec2(0.0, 1.0), w * 0.2, color);
        }
        Icon::Save => {
            let body = Rect::from_min_max(at(0.14, 0.14), at(0.86, 0.86));
            p.add(Shape::line(
                vec![body.left_top(), pos2(body.right() - w * 0.14, body.top()), pos2(body.right(), body.top() + w * 0.14), body.right_bottom(), body.left_bottom(), body.left_top()],
                s,
            ));
            p.rect_stroke(Rect::from_min_max(at(0.3, 0.14), at(0.66, 0.36)), 0.0, s, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(0.28, 0.54), at(0.72, 0.86)), 0.0, s, StrokeKind::Middle);
        }
        Icon::Pdf => {
            let page = document(p, r, s);
            p.text(page.center() + vec2(0.0, w * 0.1), Align2::CENTER_CENTER, "PDF", FontId::proportional(w * 0.22), color);
        }
    }
}

/// The LumoDent mark: a crescent of light over a disc, the name and the developer.
pub fn logo(ui: &mut Ui, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    let c = pos2(rect.center().x, rect.top() + 24.0);
    p.circle_filled(c, 19.0, theme::accent());
    p.circle_filled(c + vec2(5.0, -4.0), 16.0, theme::back());
    let font = FontId::proportional(26.0);
    let lumo = p.layout_no_wrap("Lumo".into(), font.clone(), theme::text());
    let dent = p.layout_no_wrap("Dent".into(), font, theme::accent_light());
    let x = rect.center().x - (lumo.size().x + dent.size().x) / 2.0;
    let y = rect.top() + 46.0;
    let lumo_w = lumo.size().x;
    p.galley(pos2(x, y), lumo, theme::text());
    p.galley(pos2(x + lumo_w, y), dent, theme::accent_light());
    p.text(
        pos2(rect.center().x, y + 34.0),
        Align2::CENTER_TOP,
        format!("by {}", lumodent::DEVELOPER),
        FontId::proportional(12.0),
        theme::muted(),
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Icon above the label, for toolbars and navigation tabs.
    Stacked,
    /// Icon left of the label, for buttons in side panels.
    Inline,
}

/// A themed button with a vector icon. Disabled buttons do not react.
pub fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, size: Vec2, layout: Layout, enabled: bool, selected: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
    if ui.is_rect_visible(rect) {
        let fill = if !enabled {
            theme::disabled()
        } else if selected {
            theme::accent()
        } else if response.is_pointer_button_down_on() {
            theme::button_down()
        } else if response.hovered() {
            theme::button_hover()
        } else {
            theme::button()
        };
        let text = if enabled { theme::text() } else { theme::muted() };
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(3), fill);
        p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, theme::edge()), StrokeKind::Inside);
        let font = FontId::proportional(if layout == Layout::Stacked && size.y < 60.0 { 13.0 } else { 15.0 });
        match (icon, layout) {
            (Some(icon), Layout::Stacked) => {
                let side = (rect.height() * 0.42).min(28.0);
                let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 6.0 + side / 2.0), vec2(side, side));
                paint(p, icon_rect, icon, text);
                p.text(pos2(rect.center().x, rect.bottom() - 5.0), Align2::CENTER_BOTTOM, label, font, text);
            }
            (Some(icon), Layout::Inline) => {
                let side = (rect.height() - 10.0).min(22.0);
                let icon_rect = Rect::from_center_size(pos2(rect.left() + 8.0 + side / 2.0, rect.center().y), vec2(side, side));
                paint(p, icon_rect, icon, text);
                p.text(pos2(icon_rect.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, label, font, text);
            }
            (None, _) => {
                p.text(rect.center(), Align2::CENTER_CENTER, label, font, text);
            }
        }
    }
    if enabled {
        response
    } else {
        response.on_hover_cursor(egui::CursorIcon::NotAllowed)
    }
}
