//! Image properties: file facts, image facts and EXIF details.

use std::path::Path;

use chrono::{DateTime, Local};

#[derive(Clone, Default, Debug)]
pub struct Exif {
    pub orientation: u32,
    pub date_taken: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub exposure: Option<String>,
    pub f_number: Option<String>,
    pub iso: Option<String>,
    pub focal_length: Option<String>,
    pub creator: Option<String>,
    pub copyright: Option<String>,
    pub software: Option<String>,
    pub comment: Option<String>,
    /// Latitude, longitude in decimal degrees.
    pub gps: Option<(f64, f64)>,
    pub altitude: Option<String>,
}

#[derive(Clone, Default, Debug)]
pub struct Meta {
    pub file_name: String,
    pub folder: Option<String>,
    pub file_size: Option<u64>,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub color: Option<String>,
    pub alpha: Option<bool>,
    pub bit_depth: Option<u8>,
    pub frames: usize,
    pub exif: Exif,
}

fn fmt_time(t: std::time::SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["bytes", "kB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

pub fn file_facts(path: &Path, m: &mut Meta) {
    m.file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    m.folder = path.parent().map(|p| p.display().to_string());
    if let Ok(md) = std::fs::metadata(path) {
        m.file_size = Some(md.len());
        m.created = md.created().ok().map(fmt_time);
        m.modified = md.modified().ok().map(fmt_time);
    }
}

fn text(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    let f = exif.get_field(tag, exif::In::PRIMARY)?;
    let s = match &f.value {
        exif::Value::Ascii(v) => v
            .iter()
            .map(|b| String::from_utf8_lossy(b).trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        _ => f.display_value().with_unit(exif).to_string(),
    };
    let s = s.trim().trim_matches('"').trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn rational(exif: &exif::Exif, tag: exif::Tag) -> Option<f64> {
    match &exif.get_field(tag, exif::In::PRIMARY)?.value {
        exif::Value::Rational(v) => v.first().map(|r| r.to_f64()),
        exif::Value::SRational(v) => v.first().map(|r| r.to_f64()),
        _ => None,
    }
}

fn gps_coord(exif: &exif::Exif, tag: exif::Tag, ref_tag: exif::Tag) -> Option<f64> {
    let f = exif.get_field(tag, exif::In::PRIMARY)?;
    let exif::Value::Rational(v) = &f.value else { return None };
    if v.len() < 3 {
        return None;
    }
    let mut deg = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    if let Some(r) = text(exif, ref_tag) {
        if r.starts_with('S') || r.starts_with('W') {
            deg = -deg;
        }
    }
    deg.is_finite().then_some(deg)
}

pub fn read_exif(path: &Path) -> Exif {
    let mut out = Exif { orientation: 1, ..Default::default() };
    let Ok(file) = std::fs::File::open(path) else { return out };
    let mut reader = std::io::BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else { return out };
    use exif::Tag;

    if let Some(f) = exif.get_field(Tag::Orientation, exif::In::PRIMARY) {
        out.orientation = f.value.get_uint(0).unwrap_or(1);
    }
    out.date_taken = text(&exif, Tag::DateTimeOriginal)
        .or_else(|| text(&exif, Tag::DateTime))
        .map(|s| {
            // EXIF dates look like "2024:05:01 13:45:10".
            let b = s.as_bytes();
            if b.len() >= 16 && b[4] == b':' && b[7] == b':' {
                format!("{}-{}-{} {}", &s[0..4], &s[5..7], &s[8..10], &s[11..16])
            } else {
                s
            }
        });
    let make = text(&exif, Tag::Make);
    let model = text(&exif, Tag::Model);
    out.camera = match (make, model) {
        (Some(mk), Some(md)) if md.to_lowercase().starts_with(&mk.to_lowercase()) => Some(md),
        (Some(mk), Some(md)) => Some(format!("{mk} {md}")),
        (a, b) => a.or(b),
    };
    out.lens = text(&exif, Tag::LensModel);
    out.exposure = rational(&exif, Tag::ExposureTime).map(|t| {
        if t > 0.0 && t < 1.0 {
            format!("1/{:.0} s", 1.0 / t)
        } else {
            format!("{t:.1} s")
        }
    });
    out.f_number = rational(&exif, Tag::FNumber).map(|f| format!("f/{f:.1}"));
    out.iso = exif
        .get_field(Tag::PhotographicSensitivity, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .map(|v| format!("ISO {v}"));
    out.focal_length = rational(&exif, Tag::FocalLength).map(|f| format!("{f:.0} mm"));
    out.creator = text(&exif, Tag::Artist);
    out.copyright = text(&exif, Tag::Copyright);
    out.software = text(&exif, Tag::Software);
    out.comment = text(&exif, Tag::ImageDescription);
    let lat = gps_coord(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef);
    let lon = gps_coord(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef);
    if let (Some(lat), Some(lon)) = (lat, lon) {
        out.gps = Some((lat, lon));
    }
    out.altitude = rational(&exif, Tag::GPSAltitude).map(|a| format!("{a:.0} m"));
    out
}

pub fn format_coords((lat, lon): (f64, f64)) -> String {
    let ns = if lat >= 0.0 { "N" } else { "S" };
    let ew = if lon >= 0.0 { "E" } else { "W" };
    format!("{:.5}° {ns}, {:.5}° {ew}", lat.abs(), lon.abs())
}

pub fn map_url((lat, lon): (f64, f64)) -> String {
    format!("https://www.openstreetmap.org/?mlat={lat:.6}&mlon={lon:.6}#map=15/{lat:.6}/{lon:.6}")
}

pub fn describe_color(c: image::ColorType) -> (String, bool, u8) {
    use image::ColorType::*;
    let (name, alpha, depth) = match c {
        L8 => ("Grayscale", false, 8),
        La8 => ("Grayscale", true, 8),
        Rgb8 => ("RGB", false, 8),
        Rgba8 => ("RGB", true, 8),
        L16 => ("Grayscale", false, 16),
        La16 => ("Grayscale", true, 16),
        Rgb16 => ("RGB", false, 16),
        Rgba16 => ("RGB", true, 16),
        Rgb32F => ("RGB (float)", false, 32),
        Rgba32F => ("RGB (float)", true, 32),
        _ => ("Unknown", c.has_alpha(), (c.bits_per_pixel() / c.channel_count() as u16) as u8),
    };
    (name.to_string(), alpha, depth)
}
