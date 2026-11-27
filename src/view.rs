//! Zoom, pan and rotation of the displayed image, with smooth animation.
//!
//! Zoom is measured in physical pixels per image pixel, so 1.0 means
//! "100%": one image pixel per screen pixel.

use eframe::egui::{
    self, emath::Rot2, epaint::Vertex, Color32, Mesh, Painter, Pos2, Rect, TextureId, Vec2,
};
use std::f32::consts::FRAC_PI_2;

pub const MAX_ZOOM: f32 = 20.0;
pub const ZOOM_STEP_BUTTON: f32 = 1.5;
pub const ZOOM_STEP_WHEEL: f32 = 1.3;

pub struct View {
    pub zoom: f32,
    pub target_zoom: f32,
    pub best_fit: bool,
    /// Image centre relative to viewport centre, in points.
    pub offset: Vec2,
    /// Target rotation in quarter turns clockwise.
    pub quarter: i32,
    /// Displayed rotation in radians (animated towards `quarter`).
    pub angle: f32,
    /// Screen point that stays fixed during a zoom, and the image-space
    /// vector from the image centre to it (in points at zoom 1).
    anchor: Option<(Pos2, Vec2)>,
    pub viewport: Rect,
    pub ppp: f32,
    /// Real image size in pixels.
    pub image_size: Vec2,
    /// Kinetic panning speed in points per second.
    velocity: Vec2,
}

impl Default for View {
    fn default() -> Self {
        View {
            zoom: 1.0,
            target_zoom: 1.0,
            best_fit: true,
            offset: Vec2::ZERO,
            quarter: 0,
            angle: 0.0,
            anchor: None,
            viewport: Rect::NOTHING,
            ppp: 1.0,
            image_size: Vec2::new(1.0, 1.0),
            velocity: Vec2::ZERO,
        }
    }
}

impl View {
    /// A fresh view that keeps the viewport size.
    pub fn fresh(&self) -> Self {
        View { viewport: self.viewport, ppp: self.ppp, ..Default::default() }
    }

    pub fn reset(&mut self, image_size: Vec2) {
        *self = View { viewport: self.viewport, ppp: self.ppp, image_size, ..Default::default() };
        let f = self.fit_zoom();
        self.zoom = f;
        self.target_zoom = f;
    }

    fn rotated_size(&self, quarter: i32) -> Vec2 {
        if quarter.rem_euclid(2) == 1 {
            Vec2::new(self.image_size.y, self.image_size.x)
        } else {
            self.image_size
        }
    }

    /// Largest zoom that shows the whole image, never above 100%.
    pub fn fit_zoom(&self) -> f32 {
        let s = self.rotated_size(self.quarter);
        let vp = self.viewport.size() * self.ppp;
        if vp.x <= 0.0 || vp.y <= 0.0 {
            return 1.0;
        }
        (vp.x / s.x).min(vp.y / s.y).min(1.0)
    }

    pub fn min_zoom(&self) -> f32 {
        self.fit_zoom().min(1.0)
    }

    pub fn is_animating(&self) -> bool {
        (self.zoom / self.target_zoom).ln().abs() > 1e-4
            || self.velocity != Vec2::ZERO
            || (self.angle - self.quarter as f32 * FRAC_PI_2).abs() > 1e-4
    }

    fn center(&self) -> Pos2 {
        self.viewport.center() + self.offset
    }

    /// Displayed size (rotated bounding box) in points.
    fn displayed_size(&self, zoom: f32) -> Vec2 {
        self.rotated_size(self.quarter) * zoom / self.ppp
    }

    pub fn zoom_to(&mut self, target: f32, around: Option<Pos2>) {
        let min = self.min_zoom();
        let target = target.clamp(min, MAX_ZOOM);
        let p = around
            .filter(|p| self.viewport.contains(*p))
            .unwrap_or(self.viewport.center());
        self.anchor = Some((p, (p - self.center()) / self.zoom));
        self.target_zoom = target;
        self.best_fit = (target - self.fit_zoom()).abs() < 1e-4;
    }

    pub fn zoom_by(&mut self, factor: f32, around: Option<Pos2>) {
        self.zoom_to(self.target_zoom * factor, around);
    }

    pub fn zoom_best_fit(&mut self) {
        self.zoom_to(self.fit_zoom(), None);
        self.best_fit = true;
        self.anchor = None;
    }

    pub fn rotate(&mut self, quarters: i32) {
        self.quarter += quarters;
        if self.best_fit {
            self.target_zoom = self.fit_zoom();
            self.anchor = None;
        } else {
            self.target_zoom = self.target_zoom.max(self.min_zoom());
        }
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.velocity = Vec2::ZERO;
        self.offset += delta;
        self.clamp_offset();
    }

    /// Start a kinetic glide after a drag is released.
    pub fn fling(&mut self, velocity: Vec2) {
        if self.can_pan() && velocity.length() > 60.0 {
            self.velocity = velocity.clamp(Vec2::splat(-6000.0), Vec2::splat(6000.0));
        }
    }

    pub fn can_pan(&self) -> bool {
        let s = self.displayed_size(self.zoom);
        s.x > self.viewport.width() + 0.5 || s.y > self.viewport.height() + 0.5
    }

    fn clamp_offset(&mut self) {
        let s = self.displayed_size(self.zoom);
        let vp = self.viewport.size();
        for i in 0..2 {
            let slack = ((s[i] - vp[i]) / 2.0).max(0.0);
            self.offset[i] = self.offset[i].clamp(-slack, slack);
        }
    }

    /// Advance animations; call once per frame before drawing.
    pub fn update(&mut self, viewport: Rect, ppp: f32, dt: f32) {
        let resized = viewport != self.viewport || ppp != self.ppp;
        self.viewport = viewport;
        self.ppp = ppp;
        if resized && self.best_fit {
            let f = self.fit_zoom();
            self.zoom = f;
            self.target_zoom = f;
        }
        let k = 1.0 - (-dt * 16.0).exp();
        if (self.zoom / self.target_zoom).ln().abs() > 1e-4 {
            self.zoom *= (self.target_zoom / self.zoom).powf(k);
        } else {
            self.zoom = self.target_zoom;
        }
        let ta = self.quarter as f32 * FRAC_PI_2;
        if (self.angle - ta).abs() > 1e-4 {
            self.angle += (ta - self.angle) * k;
        } else {
            self.angle = ta;
        }
        if self.velocity != Vec2::ZERO {
            if self.anchor.is_some() {
                self.velocity = Vec2::ZERO;
            } else {
                let before = self.offset;
                self.offset += self.velocity * dt;
                self.clamp_offset();
                // Stop along an axis that hit the edge.
                for i in 0..2 {
                    if (self.offset[i] - (before[i] + self.velocity[i] * dt)).abs() > 0.01 {
                        self.velocity[i] = 0.0;
                    }
                }
                self.velocity *= (-dt * 4.0).exp();
                if self.velocity.length() < 8.0 {
                    self.velocity = Vec2::ZERO;
                }
            }
        }
        if let Some((p, u)) = self.anchor {
            self.offset = p - self.viewport.center() - u * self.zoom;
            if self.zoom == self.target_zoom {
                self.anchor = None;
            }
        }
        self.clamp_offset();
    }

    /// Screen rect of the unrotated image.
    pub fn image_rect(&self) -> Rect {
        Rect::from_center_size(self.center(), self.image_size * self.zoom / self.ppp)
    }

    pub fn draw(&self, painter: &Painter, tex: TextureId, checker: bool, dark: bool) {
        let rect = self.image_rect();
        let rot = Rot2::from_angle(self.angle);
        if checker {
            draw_checkerboard(painter, rect, rot, self.center(), dark);
        }
        let mut mesh = Mesh::with_texture(tex);
        mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        if self.angle != 0.0 {
            mesh.rotate(rot, self.center());
        }
        painter.add(mesh);
    }
}

pub fn draw_checkerboard(painter: &Painter, rect: Rect, rot: Rot2, origin: Pos2, dark: bool) {
    let (a, b) = if dark {
        (Color32::from_gray(58), Color32::from_gray(72))
    } else {
        (Color32::from_gray(222), Color32::from_gray(245))
    };
    let mut mesh = Mesh::default();
    mesh.add_colored_rect(rect, a);
    let cell = 10.0;
    let clip = painter.clip_rect();
    let nx = (rect.width() / cell).ceil() as i32;
    let ny = (rect.height() / cell).ceil() as i32;
    // Avoid enormous meshes when zoomed in a lot: only cells near the view.
    if rot.angle() == 0.0 && (nx as i64 * ny as i64) > 40_000 {
        let vis = rect.intersect(clip);
        let x0 = ((vis.min.x - rect.min.x) / cell).floor() as i32;
        let y0 = ((vis.min.y - rect.min.y) / cell).floor() as i32;
        let x1 = ((vis.max.x - rect.min.x) / cell).ceil() as i32;
        let y1 = ((vis.max.y - rect.min.y) / cell).ceil() as i32;
        add_cells(&mut mesh, rect, cell, b, x0, x1, y0, y1);
    } else if (nx as i64 * ny as i64) <= 40_000 {
        add_cells(&mut mesh, rect, cell, b, 0, nx, 0, ny);
    }
    if rot.angle() != 0.0 {
        mesh.rotate(rot, origin);
    }
    painter.add(mesh);
}

#[allow(clippy::too_many_arguments)]
fn add_cells(mesh: &mut Mesh, rect: Rect, cell: f32, color: Color32, x0: i32, x1: i32, y0: i32, y1: i32) {
    for y in y0..y1 {
        for x in x0..x1 {
            if (x + y) % 2 == 0 {
                continue;
            }
            let min = rect.min + Vec2::new(x as f32 * cell, y as f32 * cell);
            let r = Rect::from_min_size(min, Vec2::splat(cell)).intersect(rect);
            if r.is_positive() {
                mesh.add_colored_rect(r, color);
            }
        }
    }
}

/// Draw a texture into `rect` with explicit texture coordinates for each
/// corner (TL, TR, BR, BL) — used by the editor for rotate/flip previews.
pub fn draw_with_corners(painter: &Painter, tex: TextureId, rect: Rect, uv: [Pos2; 4]) {
    let mut mesh = Mesh::with_texture(tex);
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    for i in 0..4 {
        mesh.vertices.push(Vertex { pos: corners[i], uv: uv[i], color: Color32::WHITE });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(mesh);
}
