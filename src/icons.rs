//! Small vector icons drawn with the painter, so they look the same on every
//! system regardless of installed fonts.

use eframe::egui::{self, pos2, vec2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use std::f32::consts::PI;

#[derive(Clone, Copy)]
pub enum Icon {
    Menu,
    Info,
    Edit,
    Open,
    RotateLeft,
    RotateRight,
    Prev,
    Next,
    FlipH,
    FlipV,
    Warning,
    ZoomIn,
    ZoomOut,
}

fn arc(p: &Painter, c: Pos2, r: f32, a0: f32, a1: f32, s: Stroke) -> Pos2 {
    let n = 20;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    let end = *pts.last().unwrap();
    p.add(Shape::line(pts, s));
    end
}

fn arrow_head(p: &Painter, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
    let d = dir.normalized();
    let n = vec2(-d.y, d.x);
    let base = tip - d * size;
    p.add(Shape::convex_polygon(vec![tip, base + n * size * 0.6, base - n * size * 0.6], color, Stroke::NONE));
}

pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let u = s / 16.0; // design grid: 16 units
    let st = Stroke::new(1.6 * u.max(0.8), color);
    match icon {
        Icon::Menu => {
            for dy in [-4.0, 0.0, 4.0] {
                p.line_segment([c + vec2(-5.5 * u, dy * u), c + vec2(5.5 * u, dy * u)], st);
            }
        }
        Icon::Info => {
            p.circle_stroke(c, 6.5 * u, st);
            p.circle_filled(c + vec2(0.0, -3.0 * u), 1.1 * u, color);
            p.line_segment([c + vec2(0.0, -0.5 * u), c + vec2(0.0, 3.8 * u)], st);
        }
        Icon::Edit => {
            let a = c + vec2(-5.0 * u, 5.0 * u);
            let b = c + vec2(4.0 * u, -4.0 * u);
            p.line_segment([a + vec2(1.5 * u, 0.0), b + vec2(1.2 * u, 1.2 * u)], st);
            p.line_segment([a + vec2(0.0, -1.5 * u), b - vec2(1.2 * u, 1.2 * u)], st);
            p.line_segment([b + vec2(1.2 * u, 1.2 * u), b - vec2(1.2 * u, 1.2 * u)], st);
            p.line_segment([a + vec2(1.5 * u, 0.0), a + vec2(0.0, -1.5 * u)], st);
        }
        Icon::Open => {
            let r = Rect::from_center_size(c + vec2(0.0, 0.8 * u), vec2(13.0 * u, 9.0 * u));
            p.rect_stroke(r, 1.5 * u, st, egui::StrokeKind::Middle);
            p.line_segment([r.left_top() + vec2(0.0, 0.0), r.left_top() + vec2(4.5 * u, 0.0)], st);
            p.line_segment([r.left_top() + vec2(1.0 * u, -2.0 * u), r.left_top() + vec2(4.5 * u, -2.0 * u)], st);
            p.line_segment([r.left_top() + vec2(4.5 * u, -2.0 * u), r.left_top() + vec2(6.0 * u, 0.0)], st);
        }
        Icon::RotateLeft | Icon::RotateRight => {
            let r = 5.5 * u;
            let cw = matches!(icon, Icon::RotateRight);
            let (a0, a1) = if cw { (PI * 0.85, PI * 2.25) } else { (PI * 0.15, -PI * 1.25) };
            let end = arc(p, c, r, a0, a1, st);
            let tangent = if cw { vec2(-a1.sin(), a1.cos()) } else { vec2(a1.sin(), -a1.cos()) };
            arrow_head(p, end + tangent * 2.2 * u, tangent, 3.6 * u, color);
        }
        Icon::Prev | Icon::Next => {
            let sx = if matches!(icon, Icon::Next) { 1.0 } else { -1.0 };
            let pts = vec![
                c + vec2(-2.0 * u * sx, -5.0 * u),
                c + vec2(3.0 * u * sx, 0.0),
                c + vec2(-2.0 * u * sx, 5.0 * u),
            ];
            p.add(Shape::line(pts, Stroke::new(2.2 * u, color)));
        }
        Icon::FlipH | Icon::FlipV => {
            let horiz = matches!(icon, Icon::FlipH);
            let t = |v: Vec2| if horiz { v } else { vec2(v.y, v.x) };
            p.line_segment([c + t(vec2(0.0, -7.0 * u)), c + t(vec2(0.0, 7.0 * u))], Stroke::new(1.0 * u, color.gamma_multiply(0.6)));
            let left = vec![c + t(vec2(-1.8 * u, -5.0 * u)), c + t(vec2(-1.8 * u, 5.0 * u)), c + t(vec2(-7.0 * u, 5.0 * u))];
            p.add(Shape::convex_polygon(left, color, Stroke::NONE));
            let right = vec![c + t(vec2(1.8 * u, -5.0 * u)), c + t(vec2(7.0 * u, 5.0 * u)), c + t(vec2(1.8 * u, 5.0 * u))];
            p.add(Shape::closed_line(right, st));
        }
        Icon::Warning => {
            let pts = vec![c + vec2(0.0, -7.0 * u), c + vec2(7.5 * u, 6.0 * u), c + vec2(-7.5 * u, 6.0 * u)];
            p.add(Shape::closed_line(pts, st));
            p.line_segment([c + vec2(0.0, -2.5 * u), c + vec2(0.0, 1.8 * u)], st);
            p.circle_filled(c + vec2(0.0, 3.9 * u), 0.9 * u, color);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            p.line_segment([c + vec2(-5.0 * u, 0.0), c + vec2(5.0 * u, 0.0)], st);
            if matches!(icon, Icon::ZoomIn) {
                p.line_segment([c + vec2(0.0, -5.0 * u), c + vec2(0.0, 5.0 * u)], st);
            }
        }
    }
}

/// A flat icon button in the current UI. `selected` draws it highlighted.
pub fn button(ui: &mut Ui, icon: Icon, tooltip: &str, selected: bool) -> Response {
    button_enabled(ui, icon, tooltip, selected, true)
}

pub fn button_enabled(ui: &mut Ui, icon: Icon, tooltip: &str, selected: bool, enabled: bool) -> Response {
    let size = Vec2::splat(ui.spacing().interact_size.y.max(26.0));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let vis = ui.style().interact_selectable(&resp, selected);
    if selected || resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 6.0, vis.weak_bg_fill);
    }
    let color = if enabled { vis.fg_stroke.color } else { ui.visuals().weak_text_color() };
    paint(ui.painter(), rect.shrink(5.0), icon, color);
    resp.on_hover_text(tooltip)
}

/// A button with an icon followed by a text label.
pub fn labeled(ui: &mut Ui, icon: Icon, text: &str, tooltip: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::TextStyle::Button.resolve(ui.style()), Color32::PLACEHOLDER);
    let h = ui.spacing().interact_size.y.max(26.0);
    let w = h + galley.size().x + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let vis = ui.style().interact(&resp);
    ui.painter().rect_filled(rect, 6.0, vis.weak_bg_fill);
    let icon_rect = Rect::from_min_size(rect.min, Vec2::splat(h)).shrink(6.0);
    paint(ui.painter(), icon_rect, icon, vis.fg_stroke.color);
    ui.painter().galley(pos2(rect.min.x + h - 2.0, rect.center().y - galley.size().y / 2.0), galley, vis.fg_stroke.color);
    resp.on_hover_text(tooltip)
}
