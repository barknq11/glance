//! Unit tests. Image tests generate their own files in a temp folder.

use std::path::PathBuf;

use crate::edit::{uv_corners, Edit, Op};
use crate::folder;
use eframe::egui::{pos2, Rect};
use image::{Rgba, RgbaImage};

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("glance-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn natural_sort_order() {
    let mut v: Vec<PathBuf> = ["img10.png", "IMG2.png", "img1.png", "a.png"].iter().map(PathBuf::from).collect();
    folder::sort(&mut v);
    let names: Vec<_> = v.iter().map(|p| p.to_string_lossy().to_string()).collect();
    assert_eq!(names, ["a.png", "img1.png", "IMG2.png", "img10.png"]);
}

#[test]
fn folder_lists_only_images() {
    let d = tmpdir("folder");
    for n in ["b.png", "a.jpg", "notes.txt", ".hidden.png", "c.svg"] {
        std::fs::write(d.join(n), b"x").unwrap();
    }
    let f = folder::Folder::for_dir(&d);
    let names: Vec<_> = f.files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    assert_eq!(names, ["a.jpg", "b.png", "c.svg"]);
}

#[test]
fn uv_identity_and_inverse() {
    let id = uv_corners(&[]);
    assert_eq!(uv_corners(&[Op::RotateCw, Op::RotateCcw]), id);
    assert_eq!(uv_corners(&[Op::RotateCw; 4]), id);
    assert_eq!(uv_corners(&[Op::FlipH, Op::FlipH]), id);
    // Rotating clockwise: top-left of the display shows the source's bottom-left.
    assert_eq!(uv_corners(&[Op::RotateCw])[0], pos2(0.0, 1.0));
}

/// Preview (uv corners) and the saved pixels must agree for every op sequence.
#[test]
fn edit_preview_matches_output() {
    // 3x2 image with a unique colour per pixel.
    let mut src = RgbaImage::new(3, 2);
    for (x, y, p) in src.enumerate_pixels_mut() {
        *p = Rgba([x as u8 * 80, y as u8 * 120, 7, 255]);
    }
    let seqs: Vec<Vec<Op>> = vec![
        vec![Op::RotateCw],
        vec![Op::RotateCcw],
        vec![Op::FlipH],
        vec![Op::FlipV],
        vec![Op::RotateCw, Op::FlipH],
        vec![Op::FlipV, Op::RotateCcw, Op::RotateCcw],
    ];
    for ops in seqs {
        let mut e = Edit::new([3, 2]);
        for op in &ops {
            e.push(*op);
        }
        let out = e.apply(image::DynamicImage::ImageRgba8(src.clone())).to_rgba8();
        let [w, h] = e.transformed_size();
        assert_eq!((out.width(), out.height()), (w, h), "{ops:?}");
        // Sample displayed corners via uv and compare with output corners.
        let uv = uv_corners(&ops);
        let corners = [(0, 0), (w - 1, 0), (w - 1, h - 1), (0, h - 1)];
        for i in 0..4 {
            let sx = (uv[i].x * 2.0).round() as u32; // 0 or 2 (src width-1)
            let sy = (uv[i].y * 1.0).round() as u32; // 0 or 1
            assert_eq!(out.get_pixel(corners[i].0, corners[i].1), src.get_pixel(sx, sy), "{ops:?} corner {i}");
        }
    }
}

#[test]
fn crop_follows_rotation() {
    let mut e = Edit::new([200, 100]);
    e.crop = Rect::from_min_max(pos2(0.0, 0.0), pos2(0.5, 1.0)); // left half
    e.push(Op::RotateCw); // left half becomes the top half
    assert!((e.crop.min.y - 0.0).abs() < 1e-6 && (e.crop.max.y - 0.5).abs() < 1e-6);
    let mut src = RgbaImage::from_pixel(200, 100, Rgba([0, 0, 0, 255]));
    for y in 0..100 {
        for x in 0..100 {
            src.put_pixel(x, y, Rgba([255, 0, 0, 255])); // left half red
        }
    }
    let out = e.apply(image::DynamicImage::ImageRgba8(src)).to_rgba8();
    assert_eq!((out.width(), out.height()), (100, 100));
    assert!(out.pixels().all(|p| p.0[0] == 255));
}

#[test]
fn decode_formats_and_orientation() {
    let d = tmpdir("decode");
    // PNG with alpha
    let png = d.join("a.png");
    RgbaImage::from_pixel(40, 20, Rgba([1, 2, 3, 100])).save(&png).unwrap();
    let r = crate::loader::decode(&png, 8192).unwrap();
    assert_eq!((r.meta.width, r.meta.height, r.meta.alpha), (40, 20, Some(true)));
    assert_eq!(r.meta.format, "PNG");

    // Animated GIF
    let gif = d.join("anim.gif");
    {
        let f = std::fs::File::create(&gif).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new(f);
        for i in 0..3u8 {
            let fr = image::Frame::from_parts(
                RgbaImage::from_pixel(10, 10, Rgba([i * 80, 0, 0, 255])),
                0,
                0,
                image::Delay::from_numer_denom_ms(50, 1),
            );
            enc.encode_frame(fr).unwrap();
        }
    }
    let r = crate::loader::decode(&gif, 8192).unwrap();
    assert_eq!(r.frames.len(), 3);

    // SVG
    let svg = d.join("v.svg");
    std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#).unwrap();
    let r = crate::loader::decode(&svg, 8192).unwrap();
    assert!(r.is_svg);
    assert_eq!((r.meta.width, r.meta.height), (20, 10));
    assert!(r.display_size()[0] >= 1000, "SVG should be rendered large for sharp zoom");

    // Garbage
    let bad = d.join("bad.png");
    std::fs::write(&bad, b"\x89PNG\r\n\x1a\nnope").unwrap();
    assert!(crate::loader::decode(&bad, 8192).is_err());

    // Downscale when larger than the GPU limit
    let big = d.join("big.png");
    RgbaImage::new(3000, 1000).save(&big).unwrap();
    let r = crate::loader::decode(&big, 1024).unwrap();
    assert!(r.downscaled);
    assert_eq!(r.meta.width, 3000);
    assert_eq!(r.display_size()[0], 1024);
}

/// Uses the Pillow-generated photo with EXIF orientation 6 and GPS, if present.
#[test]
fn exif_from_sample_if_available() {
    let Ok(dir) = std::env::var("GLANCE_SAMPLES") else { return };
    let p = PathBuf::from(dir).join("photo 1.jpg");
    let r = crate::loader::decode(&p, 8192).unwrap();
    assert_eq!((r.meta.width, r.meta.height), (1200, 1800), "orientation 6 should rotate");
    let e = &r.meta.exif;
    assert_eq!(e.camera.as_deref(), Some("Canon EOS R6"));
    assert_eq!(e.exposure.as_deref(), Some("1/250 s"));
    assert_eq!(e.f_number.as_deref(), Some("f/2.8"));
    assert_eq!(e.iso.as_deref(), Some("ISO 400"));
    assert_eq!(e.date_taken.as_deref(), Some("2024-05-01 13:45"));
    let (lat, lon) = e.gps.unwrap();
    assert!((lat - 48.8567).abs() < 1e-3 && (lon - 2.2944).abs() < 1e-3, "{lat} {lon}");
}

#[test]
fn copy_names() {
    let d = tmpdir("copy");
    let p = d.join("pic.jpg");
    assert_eq!(crate::edit::copy_name(&p), d.join("pic (edited).jpg"));
    std::fs::write(d.join("pic (edited).jpg"), b"x").unwrap();
    assert_eq!(crate::edit::copy_name(&p), d.join("pic (edited 2).jpg"));
}

#[test]
fn file_uri_escapes() {
    let d = tmpdir("uri");
    let p = d.join("a b#c.png");
    std::fs::write(&p, b"x").unwrap();
    let u = crate::platform::file_uri(&p);
    assert!(u.starts_with("file:///"));
    assert!(u.ends_with("a%20b%23c.png"), "{u}");
}

#[test]
fn save_edited_images() {
    let d = tmpdir("save");
    let src = d.join("pic.jpg");
    image::RgbImage::from_pixel(64, 32, image::Rgb([200, 10, 10])).save(&src).unwrap();
    let mut e = Edit::new([64, 32]);
    e.push(Op::RotateCw);
    e.crop = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 0.5));
    let out = e.apply(crate::loader::decode_full(&src).unwrap());
    // Save as copy keeps JPEG
    let dest = crate::edit::save(&out, &crate::edit::copy_name(&src)).unwrap();
    assert_eq!(dest.file_name().unwrap(), "pic (edited).jpg");
    let back = image::open(&dest).unwrap();
    assert_eq!((back.width(), back.height()), (32, 32));
    // Overwrite in place leaves no temp files behind
    crate::edit::save(&out, &src).unwrap();
    assert_eq!(image::open(&src).unwrap().width(), 32);
    let leftovers: Vec<_> = std::fs::read_dir(&d).unwrap().flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("glance-tmp")).collect();
    assert!(leftovers.is_empty());
    // SVG can't be written: saved as PNG next to it
    let svg = d.join("v.svg");
    let p = crate::edit::save(&out, &svg).unwrap();
    assert_eq!(p.extension().unwrap(), "png");
}
