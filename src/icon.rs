//! The app icon, drawn in code so there are no binary assets to ship.
//! A rounded tile with a sunrise over two hills.

use image::{Rgba, RgbaImage};

fn smooth(edge: f32, d: f32) -> f32 {
    // Coverage for a signed distance `d` (negative = inside), `edge` = 1px.
    (0.5 - d / edge).clamp(0.0, 1.0)
}

fn rounded_rect_sd(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - (hw - r);
    let qy = (y - cy).abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn blend(dst: &mut [f32; 4], src: [f32; 3], a: f32) {
    for i in 0..3 {
        dst[i] = dst[i] * (1.0 - a) + src[i] * a;
    }
    dst[3] = dst[3] + a * (1.0 - dst[3]);
}

pub fn render(size: u32) -> RgbaImage {
    let s = size as f32;
    let px = 1.0; // anti-aliasing width in pixels
    let mut img = RgbaImage::new(size, size);
    for yi in 0..size {
        for xi in 0..size {
            let x = xi as f32 + 0.5;
            let y = yi as f32 + 0.5;
            let mut c = [0.0f32; 4];

            // Tile with a vertical gradient (deep teal -> violet).
            let tile = rounded_rect_sd(x, y, s / 2.0, s / 2.0, s * 0.46, s * 0.46, s * 0.2);
            let t = y / s;
            let top = [0.13, 0.55, 0.78];
            let bottom = [0.36, 0.25, 0.67];
            let grad = [
                top[0] + (bottom[0] - top[0]) * t,
                top[1] + (bottom[1] - top[1]) * t,
                top[2] + (bottom[2] - top[2]) * t,
            ];
            let a_tile = smooth(px, tile);
            if a_tile <= 0.0 {
                continue;
            }
            blend(&mut c, grad, 1.0);

            // Sun.
            let (sx, sy, sr) = (s * 0.64, s * 0.38, s * 0.12);
            let d_sun = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt() - sr;
            blend(&mut c, [1.0, 0.82, 0.35], smooth(px, d_sun));

            // Back hill.
            let h1 = s * 0.70 - (s * 0.16) * ((x / s) * std::f32::consts::PI * 1.2 + 0.4).sin();
            blend(&mut c, [0.55, 0.80, 0.93], smooth(px, h1 - y));

            // Front hill.
            let h2 = s * 0.80 - (s * 0.13) * ((x / s) * std::f32::consts::PI * 1.6 + 2.2).sin();
            blend(&mut c, [0.95, 0.97, 1.0], smooth(px, h2 - y));

            let a = a_tile;
            img.put_pixel(
                xi,
                yi,
                Rgba([
                    (c[0] * 255.0) as u8,
                    (c[1] * 255.0) as u8,
                    (c[2] * 255.0) as u8,
                    (a * 255.0) as u8,
                ]),
            );
        }
    }
    img
}
