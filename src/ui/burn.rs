//! Annotations as egui shapes, used both to paint them on screen and to burn
//! them into an image's pixels for Send to Report and Export. Burning
//! tessellates the shapes with egui (text uses the same font atlas as the
//! screen) and fills the triangles in software.

use super::viewer::Mark;
use eframe::egui::{self, epaint, pos2, vec2, Color32, Pos2, Rect, Shape};
use lumodent::image::Image;
use lumodent::settings::{Rgba, Settings, Stroke};

fn rgba(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Shapes for the marks and rulers. `to_target` maps image pixels to where
/// they are drawn; `k` scales line widths and text (1 on screen, 1 / zoom when
/// burning so the result looks the way it did on screen).
pub fn annotation_shapes(
    ctx: &egui::Context,
    st: &Settings,
    marks: &[Mark],
    measures: &[[Pos2; 2]],
    mm_per_px: f64,
    to_target: &dyn Fn(Pos2) -> Pos2,
    k: f32,
) -> Vec<Shape> {
    let pen = |s: Stroke| egui::Stroke::new(s.thickness as f32 * k, rgba(s.color));
    let layout = |text: String, size: f32, color: Color32, wrap: f32| {
        ctx.fonts_mut(|f| f.layout(text, egui::FontId::proportional(size * k), color, wrap))
    };
    let mut out = Vec::new();
    for mark in marks {
        match mark {
            Mark::Line([a, b]) => out.push(Shape::line_segment([to_target(*a), to_target(*b)], pen(st.line))),
            Mark::Rectangle([a, b]) => out.push(Shape::rect_stroke(
                Rect::from_two_pos(to_target(*a), to_target(*b)),
                0.0,
                pen(st.rectangle),
                egui::StrokeKind::Middle,
            )),
            Mark::Ellipse([a, b]) => {
                let r = Rect::from_two_pos(to_target(*a), to_target(*b));
                out.push(epaint::EllipseShape::stroke(r.center(), r.size() / 2.0, pen(st.ellipse)).into());
            }
            Mark::Draw(points) => out.push(Shape::line(points.iter().map(|q| to_target(*q)).collect(), pen(st.draw))),
            Mark::Memo { at, text } => {
                let galley = layout(text.clone(), 14.0, rgba(st.memo_text), 320.0 * k);
                let at = to_target(*at);
                out.push(Shape::rect_filled(Rect::from_min_size(at, galley.size()).expand(5.0 * k), 4.0 * k, rgba(st.memo_outer)));
                out.push(Shape::galley(at, galley, rgba(st.memo_text)));
            }
        }
    }
    let ruler = rgba(st.length.color);
    let width = st.length.thickness as f32 * k;
    for line in measures {
        let (a, b) = (to_target(line[0]), to_target(line[1]));
        out.push(Shape::line_segment([a, b], egui::Stroke::new(width + 1.5 * k, Color32::from_black_alpha(160))));
        out.push(Shape::line_segment([a, b], egui::Stroke::new(width, ruler)));
        out.push(Shape::circle_filled(a, width + 1.5 * k, ruler));
        out.push(Shape::circle_filled(b, width + 1.5 * k, ruler));
        let mm = line[0].distance(line[1]) as f64 * mm_per_px;
        let text = if st.length_unit { format!("{mm:.1} mm") } else { format!("{mm:.1}") };
        let at = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0) + vec2(8.0, -8.0) * k;
        let galley = layout(text, 13.0, ruler, f32::INFINITY);
        let bg = Rect::from_min_size(at - vec2(0.0, galley.size().y), galley.size()).expand(3.0 * k);
        out.push(Shape::rect_filled(bg, 3.0 * k, Color32::from_black_alpha(170)));
        out.push(Shape::galley(bg.min + vec2(3.0, 3.0) * k, galley, ruler));
    }
    out
}

/// Draws `shapes` (in image pixel coordinates) into the image.
pub fn burn(ctx: &egui::Context, image: &mut Image, shapes: Vec<Shape>) {
    if shapes.is_empty() {
        return;
    }
    let (atlas, atlas_size) = ctx.fonts(|f| (f.image(), f.font_image_size()));
    let mut tess = epaint::Tessellator::new(1.0, epaint::TessellationOptions::default(), atlas_size, Vec::new());
    for shape in shapes {
        let mut mesh = epaint::Mesh::default();
        tess.tessellate_shape(shape, &mut mesh);
        for tri in mesh.indices.chunks_exact(3) {
            let v = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices[i as usize]);
            fill_triangle(image, &v, &atlas);
        }
    }
}

fn sample(atlas: &egui::ColorImage, uv: Pos2) -> [f32; 4] {
    let [w, h] = atlas.size;
    let x = ((uv.x * w as f32) as usize).min(w - 1);
    let y = ((uv.y * h as f32) as usize).min(h - 1);
    atlas.pixels[y * w + x].to_array().map(|c| c as f32 / 255.0)
}

/// Fills one triangle, interpolating colour and texture coordinates, and
/// blends the premultiplied result over the pixels.
fn fill_triangle(image: &mut Image, v: &[epaint::Vertex; 3], atlas: &egui::ColorImage) {
    let (p0, p1, p2) = (v[0].pos, v[1].pos, v[2].pos);
    let area = (p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y);
    if area.abs() < 1e-6 {
        return;
    }
    let (w, h) = (image.width as f32, image.height as f32);
    let x0 = p0.x.min(p1.x).min(p2.x).floor().max(0.0) as usize;
    let x1 = (p0.x.max(p1.x).max(p2.x).ceil().min(w) as usize).min(image.width);
    let y0 = p0.y.min(p1.y).min(p2.y).floor().max(0.0) as usize;
    let y1 = (p0.y.max(p1.y).max(p2.y).ceil().min(h) as usize).min(image.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = pos2(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((p1.x - p.x) * (p2.y - p.y) - (p2.x - p.x) * (p1.y - p.y)) / area;
            let w1 = ((p2.x - p.x) * (p0.y - p.y) - (p0.x - p.x) * (p2.y - p.y)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                continue;
            }
            let uv = pos2(
                w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x,
                w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y,
            );
            let c = [0, 1, 2, 3].map(|i| {
                (w0 * v[0].color.to_array()[i] as f32 + w1 * v[1].color.to_array()[i] as f32 + w2 * v[2].color.to_array()[i] as f32) / 255.0
            });
            let t = sample(atlas, uv);
            let src = [c[0] * t[0], c[1] * t[1], c[2] * t[2], c[3] * t[3]];
            if src[3] <= 0.0 {
                continue;
            }
            let px = &mut image.pixels[y * image.width + x];
            let dst = [(*px >> 16) & 0xff, (*px >> 8) & 0xff, *px & 0xff].map(|c| c as f32 / 255.0);
            let out = [0, 1, 2].map(|i| ((src[i] + dst[i] * (1.0 - src[3])).clamp(0.0, 1.0) * 255.0).round() as u32);
            *px = out[0] << 16 | out[1] << 8 | out[2];
        }
    }
}
