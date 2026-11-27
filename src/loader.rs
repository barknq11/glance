//! Decoding images on background threads, with a small cache so the
//! neighbouring images are ready before the user flips to them.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use eframe::egui::ColorImage;
use image::{AnimationDecoder, DynamicImage, ImageFormat};

use crate::meta::{self, Meta};

pub struct Frame {
    pub image: Arc<ColorImage>,
    pub delay: Duration,
}

pub struct Decoded {
    pub frames: Vec<Frame>,
    pub meta: Meta,
    pub is_svg: bool,
    pub mtime: Option<SystemTime>,
    /// True when the picture was too big for the GPU and was scaled down for display.
    pub downscaled: bool,
}

impl Decoded {
    /// Size of the displayed picture (may differ from the file when downscaled).
    pub fn display_size(&self) -> [usize; 2] {
        self.frames[0].image.size
    }
}

pub type LoadResult = Result<Arc<Decoded>, String>;

const MAX_FRAMES: usize = 2000;
const CACHE_SIZE: usize = 7;

fn fontdb() -> Arc<resvg::usvg::fontdb::Database> {
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

pub fn is_svg_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("svg") | Some("svgz")
    )
}

fn render_svg(path: &Path, target_long_side: f32, max_side: u32) -> Result<(DynamicImage, u32, u32), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut opt = resvg::usvg::Options::default();
    opt.resources_dir = path.parent().map(Path::to_path_buf);
    opt.fontdb = fontdb();
    let tree = resvg::usvg::Tree::from_data(&data, &opt).map_err(|e| format!("Invalid SVG: {e}"))?;
    let size = tree.size();
    let (w, h) = (size.width(), size.height());
    let long = w.max(h).max(1.0);
    let scale = (target_long_side / long).max(1.0).min(max_side as f32 / long);
    let pw = ((w * scale).ceil() as u32).max(1);
    let ph = ((h * scale).ceil() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pw, ph).ok_or("SVG is too large")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia is premultiplied; convert to straight alpha.
    let mut rgba = pixmap.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    let img = image::RgbaImage::from_raw(pw, ph, rgba).ok_or("SVG render failed")?;
    Ok((DynamicImage::ImageRgba8(img), w.round() as u32, h.round() as u32))
}

fn to_color_image(img: &DynamicImage) -> ColorImage {
    let rgba = img.to_rgba8();
    ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
}

fn fit_max_side(img: DynamicImage, max_side: u32) -> (DynamicImage, bool) {
    if img.width() > max_side || img.height() > max_side {
        (img.resize(max_side, max_side, image::imageops::FilterType::Triangle), true)
    } else {
        (img, false)
    }
}

fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Qoi => "QOI",
        ImageFormat::Hdr => "Radiance HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Dds => "DDS",
        ImageFormat::Farbfeld => "Farbfeld",
        ImageFormat::Avif => "AVIF",
        _ => "Image",
    }
}

fn reader(path: &Path) -> Result<image::ImageReader<BufReader<std::fs::File>>, String> {
    let mut r = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(4 * 1024 * 1024 * 1024);
    r.limits(limits);
    Ok(r)
}

/// Decode only the frames of an animation, if the file is animated.
fn animation_frames(path: &Path, format: ImageFormat, max_side: u32) -> Option<Vec<Frame>> {
    let open = || std::fs::File::open(path).ok().map(BufReader::new);
    let frames: image::Frames = match format {
        ImageFormat::Gif => image::codecs::gif::GifDecoder::new(open()?).ok()?.into_frames(),
        ImageFormat::WebP => {
            let d = image::codecs::webp::WebPDecoder::new(open()?).ok()?;
            if !d.has_animation() {
                return None;
            }
            d.into_frames()
        }
        ImageFormat::Png => {
            let d = image::codecs::png::PngDecoder::new(open()?).ok()?;
            if !d.is_apng().ok()? {
                return None;
            }
            d.apng().ok()?.into_frames()
        }
        _ => return None,
    };
    let mut out = Vec::new();
    for f in frames.take(MAX_FRAMES) {
        let f = f.ok()?;
        let (n, d) = f.delay().numer_denom_ms();
        let ms = if d == 0 { 100 } else { n / d };
        // Browsers treat very short delays as 100 ms; so do we.
        let ms = if ms < 20 { 100 } else { ms };
        let (img, _) = fit_max_side(DynamicImage::ImageRgba8(f.into_buffer()), max_side);
        out.push(Frame { image: Arc::new(to_color_image(&img)), delay: Duration::from_millis(ms as u64) });
    }
    (out.len() > 1).then_some(out)
}

pub fn decode(path: &Path, max_side: u32) -> Result<Decoded, String> {
    let mut m = Meta::default();
    meta::file_facts(path, &mut m);
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

    if is_svg_path(path) {
        let (img, w, h) = render_svg(path, 2048.0, max_side)?;
        m.format = "SVG".into();
        m.width = w;
        m.height = h;
        m.color = Some("Vector".into());
        m.alpha = Some(true);
        m.frames = 1;
        return Ok(Decoded {
            frames: vec![Frame { image: Arc::new(to_color_image(&img)), delay: Duration::ZERO }],
            meta: m,
            is_svg: true,
            mtime,
            downscaled: false,
        });
    }

    let r = reader(path)?;
    let format = r.format().ok_or("Unknown or unsupported image format")?;
    m.format = format_name(format).into();
    m.exif = meta::read_exif(path);

    if let Some(frames) = animation_frames(path, format, max_side) {
        let first = image::ImageReader::open(path)
            .ok()
            .and_then(|r| r.with_guessed_format().ok())
            .and_then(|r| r.into_dimensions().ok());
        let (w, h) = first.unwrap_or((frames[0].image.size[0] as u32, frames[0].image.size[1] as u32));
        m.width = w;
        m.height = h;
        m.color = Some("RGB".into());
        m.alpha = Some(true);
        m.bit_depth = Some(8);
        m.frames = frames.len();
        return Ok(Decoded { frames, meta: m, is_svg: false, mtime, downscaled: false });
    }

    let mut img = r.decode().map_err(|e| e.to_string())?;
    let (color, alpha, depth) = meta::describe_color(img.color());
    m.color = Some(color);
    m.alpha = Some(alpha);
    m.bit_depth = Some(depth);
    m.frames = 1;
    if m.exif.orientation > 1 {
        if let Some(o) = image::metadata::Orientation::from_exif(m.exif.orientation as u8) {
            img.apply_orientation(o);
        }
    }
    m.width = img.width();
    m.height = img.height();
    let (img, downscaled) = fit_max_side(img, max_side);
    Ok(Decoded {
        frames: vec![Frame { image: Arc::new(to_color_image(&img)), delay: Duration::ZERO }],
        meta: m,
        is_svg: false,
        mtime,
        downscaled,
    })
}

/// Decode at full resolution with EXIF rotation applied (for editing,
/// copying and wallpapers). Animations give their first frame.
pub fn decode_full(path: &Path) -> Result<DynamicImage, String> {
    if is_svg_path(path) {
        return render_svg(path, 2048.0, 16384).map(|(i, _, _)| i);
    }
    let mut img = reader(path)?.decode().map_err(|e| e.to_string())?;
    let o = meta::read_exif(path).orientation;
    if o > 1 {
        if let Some(o) = image::metadata::Orientation::from_exif(o as u8) {
            img.apply_orientation(o);
        }
    }
    Ok(img)
}

/// Quick sniff so we can tell "not an image" from "broken image".
pub fn looks_like_image(path: &Path) -> bool {
    if crate::folder::is_image(path) {
        return true;
    }
    let mut buf = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .map(|n| image::guess_format(&buf[..n]).is_ok())
        .unwrap_or(false)
}

pub struct Loader {
    tx: Sender<(PathBuf, LoadResult)>,
    rx: Receiver<(PathBuf, LoadResult)>,
    pending: HashSet<PathBuf>,
    cache: HashMap<PathBuf, (LoadResult, u64)>,
    tick: u64,
    pub max_side: u32,
}

impl Loader {
    pub fn new(max_side: u32) -> Self {
        let (tx, rx) = channel();
        Loader { tx, rx, pending: HashSet::new(), cache: HashMap::new(), tick: 0, max_side }
    }

    pub fn request(&mut self, path: &Path, ctx: &eframe::egui::Context) {
        if self.cache.contains_key(path) || self.pending.contains(path) {
            return;
        }
        self.pending.insert(path.to_path_buf());
        let tx = self.tx.clone();
        let p = path.to_path_buf();
        let ctx = ctx.clone();
        let max_side = self.max_side;
        std::thread::spawn(move || {
            let res = std::panic::catch_unwind(|| decode(&p, max_side))
                .unwrap_or_else(|_| Err("The decoder crashed on this file".into()))
                .map(Arc::new);
            let _ = tx.send((p, res));
            ctx.request_repaint();
        });
    }

    /// Collect finished decodes. Returns true if anything arrived.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((p, res)) = self.rx.try_recv() {
            self.pending.remove(&p);
            self.tick += 1;
            self.cache.insert(p, (res, self.tick));
            any = true;
        }
        while self.cache.len() > CACHE_SIZE {
            let oldest = self.cache.iter().min_by_key(|(_, (_, t))| *t).map(|(p, _)| p.clone());
            if let Some(p) = oldest {
                self.cache.remove(&p);
            }
        }
        any
    }

    pub fn get(&mut self, path: &Path) -> Option<LoadResult> {
        self.tick += 1;
        let t = self.tick;
        self.cache.get_mut(path).map(|(r, used)| {
            *used = t;
            r.clone()
        })
    }

    pub fn is_pending(&self, path: &Path) -> bool {
        self.pending.contains(path)
    }

    pub fn forget(&mut self, path: &Path) {
        self.cache.remove(path);
    }
}
