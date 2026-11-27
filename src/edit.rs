//! Editing: rotate, flip and crop, then save over the file or as a copy.

use std::path::{Path, PathBuf};

use eframe::egui::{pos2, Pos2, Rect};
use image::DynamicImage;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    RotateCw,
    RotateCcw,
    FlipH,
    FlipV,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Aspect {
    Free,
    Original,
    Square,
    Ratio(f32, f32),
}

impl Aspect {
    pub const PRESETS: [(&'static str, Aspect); 7] = [
        ("Free", Aspect::Free),
        ("Original", Aspect::Original),
        ("Square", Aspect::Square),
        ("16:9", Aspect::Ratio(16.0, 9.0)),
        ("4:3", Aspect::Ratio(4.0, 3.0)),
        ("3:2", Aspect::Ratio(3.0, 2.0)),
        ("5:4", Aspect::Ratio(5.0, 4.0)),
    ];
}

/// Edit state. `crop` is in 0..1 coordinates of the *transformed* image.
#[derive(Clone)]
pub struct Edit {
    pub ops: Vec<Op>,
    pub crop: Rect,
    pub aspect: Aspect,
    pub portrait: bool,
    /// Size of the untransformed source image in pixels.
    pub src_size: [u32; 2],
}

impl Edit {
    pub fn new(src_size: [u32; 2]) -> Self {
        Edit {
            ops: vec![],
            crop: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            aspect: Aspect::Free,
            portrait: false,
            src_size,
        }
    }

    pub fn quarter_turns(&self) -> i32 {
        self.ops
            .iter()
            .map(|o| match o {
                Op::RotateCw => 1,
                Op::RotateCcw => -1,
                _ => 0,
            })
            .sum()
    }

    /// Size of the image after rotations/flips.
    pub fn transformed_size(&self) -> [u32; 2] {
        if self.quarter_turns().rem_euclid(2) == 1 {
            [self.src_size[1], self.src_size[0]]
        } else {
            self.src_size
        }
    }

    pub fn is_modified(&self) -> bool {
        let full = self.crop.min.x <= 0.0005
            && self.crop.min.y <= 0.0005
            && self.crop.max.x >= 0.9995
            && self.crop.max.y >= 0.9995;
        !full || self.quarter_turns().rem_euclid(4) != 0 || self.flips_cancel() == false
    }

    fn flips_cancel(&self) -> bool {
        // Cheap check: does the full op list map corners back to identity?
        uv_corners(&self.ops) == uv_corners(&[])
    }

    pub fn push(&mut self, op: Op) {
        // Transform the crop rectangle along with the image.
        let c = self.crop;
        self.crop = match op {
            Op::RotateCw => Rect::from_min_max(pos2(1.0 - c.max.y, c.min.x), pos2(1.0 - c.min.y, c.max.x)),
            Op::RotateCcw => Rect::from_min_max(pos2(c.min.y, 1.0 - c.max.x), pos2(c.max.y, 1.0 - c.min.x)),
            Op::FlipH => Rect::from_min_max(pos2(1.0 - c.max.x, c.min.y), pos2(1.0 - c.min.x, c.max.y)),
            Op::FlipV => Rect::from_min_max(pos2(c.min.x, 1.0 - c.max.y), pos2(c.max.x, 1.0 - c.min.y)),
        };
        if matches!(op, Op::RotateCw | Op::RotateCcw) {
            self.portrait = !self.portrait;
        }
        self.ops.push(op);
    }

    /// Target aspect (width / height) in pixels, if constrained.
    pub fn ratio(&self) -> Option<f32> {
        let [w, h] = self.transformed_size();
        let r = match self.aspect {
            Aspect::Free => return None,
            Aspect::Original => {
                let [sw, sh] = self.src_size;
                let r = sw as f32 / sh as f32;
                if self.quarter_turns().rem_euclid(2) == 1 { 1.0 / r } else { r }
            }
            Aspect::Square => 1.0,
            Aspect::Ratio(a, b) => {
                if self.portrait { b / a } else { a / b }
            }
        };
        let _ = (w, h);
        Some(r)
    }

    /// Shrink the crop around its centre to satisfy the aspect ratio.
    pub fn apply_aspect(&mut self) {
        let Some(r) = self.ratio() else { return };
        let [w, h] = self.transformed_size();
        let (w, h) = (w as f32, h as f32);
        // Largest rect with ratio r that fits the image, then fit to current crop.
        let cur_w = self.crop.width() * w;
        let cur_h = self.crop.height() * h;
        let (mut nw, mut nh) = (cur_w, cur_w / r);
        if nh > cur_h {
            nh = cur_h;
            nw = nh * r;
        }
        if nw < 1.0 || nh < 1.0 {
            (nw, nh) = if w / h > r { (h * r, h) } else { (w, w / r) };
        }
        let c = self.crop.center();
        let half = eframe::egui::vec2(nw / w / 2.0, nh / h / 2.0);
        let mut rect = Rect::from_center_size(c, half * 2.0);
        rect = rect.translate(eframe::egui::vec2(
            (-rect.min.x).max(0.0) - (rect.max.x - 1.0).max(0.0),
            (-rect.min.y).max(0.0) - (rect.max.y - 1.0).max(0.0),
        ));
        self.crop = rect;
    }

    pub fn apply(&self, mut img: DynamicImage) -> DynamicImage {
        for op in &self.ops {
            img = match op {
                Op::RotateCw => img.rotate90(),
                Op::RotateCcw => img.rotate270(),
                Op::FlipH => img.fliph(),
                Op::FlipV => img.flipv(),
            };
        }
        let (w, h) = (img.width() as f32, img.height() as f32);
        let x = (self.crop.min.x * w).round().max(0.0) as u32;
        let y = (self.crop.min.y * h).round().max(0.0) as u32;
        let cw = ((self.crop.width() * w).round() as u32).clamp(1, img.width() - x.min(img.width() - 1));
        let ch = ((self.crop.height() * h).round() as u32).clamp(1, img.height() - y.min(img.height() - 1));
        if x == 0 && y == 0 && cw == img.width() && ch == img.height() {
            img
        } else {
            img.crop_imm(x, y, cw, ch)
        }
    }
}

/// Texture coordinates for the displayed corners (TL, TR, BR, BL) after
/// applying `ops` in display space.
pub fn uv_corners(ops: &[Op]) -> [Pos2; 4] {
    let mut c = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    for op in ops {
        c = match op {
            Op::RotateCw => [c[3], c[0], c[1], c[2]],
            Op::RotateCcw => [c[1], c[2], c[3], c[0]],
            Op::FlipH => [c[1], c[0], c[3], c[2]],
            Op::FlipV => [c[3], c[2], c[1], c[0]],
        };
    }
    c
}

pub fn save_jpeg(img: &DynamicImage, dest: &Path, quality: u8) -> Result<(), String> {
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, quality);
    img.to_rgb8().write_with_encoder(enc).map_err(|e| e.to_string())
}

/// Save in the format implied by `dest`'s extension. Formats we can't write
/// (e.g. SVG) are saved as PNG.
pub fn save(img: &DynamicImage, dest: &Path) -> Result<PathBuf, String> {
    let ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut dest = dest.to_path_buf();
    // Write to a temporary file first so a failed save never destroys the original.
    let writable = image::ImageFormat::from_extension(&ext)
        .map(|f| f.writing_enabled() && !matches!(f, image::ImageFormat::Gif))
        .unwrap_or(false);
    if !writable {
        dest.set_extension("png");
    }
    let tmp = dest.with_extension(format!("{}.glance-tmp", dest.extension().unwrap().to_string_lossy()));
    let res = match ext.as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" if writable => save_jpeg(img, &tmp, 92),
        _ => {
            let fmt = image::ImageFormat::from_path(&dest).map_err(|e| e.to_string())?;
            let img = if matches!(fmt, image::ImageFormat::Bmp | image::ImageFormat::Ico) {
                DynamicImage::ImageRgba8(img.to_rgba8())
            } else {
                img.clone()
            };
            img.save_with_format(&tmp, fmt).map_err(|e| e.to_string())
        }
    };
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(dest)
}

/// "photo.jpg" -> "photo (edited).jpg", "photo (edited 2).jpg", ...
pub fn copy_name(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().to_string());
    let dir = path.parent().unwrap_or(Path::new("."));
    for n in 1.. {
        let name = if n == 1 { format!("{stem} (edited)") } else { format!("{stem} (edited {n})") };
        let p = match &ext {
            Some(e) => dir.join(format!("{name}.{e}")),
            None => dir.join(name),
        };
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}
