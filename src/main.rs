// Glance — a simple, fast image viewer for Windows and Linux.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod edit;
mod folder;
mod icon;
mod icons;
mod loader;
mod meta;
mod platform;
mod view;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--export-icon") => {
                let out = args.next().map(PathBuf::from).unwrap_or_else(|| "glance.png".into());
                let size = args
                    .next()
                    .and_then(|s| s.to_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(256);
                if let Err(e) = icon::render(size).save(&out) {
                    eprintln!("Could not write icon: {e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            Some("--version") | Some("-v") => {
                println!("Glance {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--help") | Some("-h") => {
                println!("Usage: glance [FILE|FOLDER]...\n\nOptions:\n  --export-icon PATH [SIZE]  Write the app icon as PNG\n  --version                  Show version");
                return Ok(());
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    let icon = icon::render(128);
    let icon_data = eframe::egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Glance")
            .with_app_id("io.github.glance.Glance")
            .with_inner_size([1000.0, 720.0])
            .with_min_inner_size([360.0, 300.0])
            .with_drag_and_drop(true)
            .with_icon(icon_data),
        ..Default::default()
    };

    eframe::run_native(
        "Glance",
        options,
        Box::new(move |cc| Ok(Box::new(app::GlanceApp::new(cc, files)))),
    )
}
