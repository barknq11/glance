//! The main window: header bar, image area, overlay controls, properties
//! panel, editor and dialogs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Id, Key, Layout, Modifiers, PointerButton, Pos2, Rect,
    RichText, Sense, Stroke, TextureHandle, TextureOptions, Vec2,
};

use crate::edit::{self, Aspect, Edit, Op};
use crate::folder::Folder;
use crate::icons::{self, Icon};
use crate::loader::{Decoded, Loader};
use crate::meta;
use crate::platform::{self, WallpaperStyle};
use crate::view::{self, View, ZOOM_STEP_BUTTON, ZOOM_STEP_WHEEL};

enum Dialog {
    None,
    ConfirmDelete,
    Wallpaper(WallpaperStyle),
    Shortcuts,
    About,
    OpenWith(Vec<platform::AppEntry>),
    ConfirmOverwrite,
    ConfirmPrint,
    Message { title: String, text: String },
}

struct Textures {
    path: PathBuf,
    data: Arc<Decoded>,
    linear: Vec<Option<TextureHandle>>,
    nearest: Vec<Option<TextureHandle>>,
}

impl Textures {
    fn get(&mut self, ctx: &egui::Context, i: usize, nearest: bool) -> egui::TextureId {
        let (slot, opts, tag) = if nearest {
            (&mut self.nearest, TextureOptions::NEAREST, "n")
        } else {
            (
                &mut self.linear,
                TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR },
                "l",
            )
        };
        if slot[i].is_none() {
            let img = self.data.frames[i].image.clone();
            slot[i] = Some(ctx.load_texture(
                format!("{}#{i}{tag}", self.path.display()),
                egui::ImageData::Color(img),
                opts,
            ));
        }
        slot[i].as_ref().unwrap().id()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Move,
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

struct EditSession {
    edit: Edit,
    drag: Option<(Handle, Rect)>,
}

pub struct GlanceApp {
    folder: Folder,
    current: Option<PathBuf>,
    loader: Loader,
    tex: Option<Textures>,
    view: View,
    frame_idx: usize,
    frame_started: f64,
    show_props: bool,
    fullscreen: bool,
    dialog: Dialog,
    toast: Option<(String, f64)>,
    edit: Option<EditSession>,
    last_activity: f64,
    last_scan: f64,
    load_started: f64,
    clipboard: Option<arboard::Clipboard>,
    configured_max_side: bool,
    last_title: String,
    scan_rx: Option<std::sync::mpsc::Receiver<Option<Vec<PathBuf>>>>,
    /// Debug builds: scripted screenshots for testing (GLANCE_SHOTS=dir).
    shots: Option<(PathBuf, usize, f64)>,
}

impl GlanceApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::System);
        cc.egui_ctx.all_styles_mut(|s| {
            s.spacing.button_padding = Vec2::new(8.0, 4.0);
            s.visuals.window_corner_radius = CornerRadius::same(12);
            s.visuals.menu_corner_radius = CornerRadius::same(8);
        });
        let mut app = GlanceApp {
            folder: Folder::default(),
            current: None,
            loader: Loader::new(8192),
            tex: None,
            view: View::default(),
            frame_idx: 0,
            frame_started: 0.0,
            show_props: false,
            fullscreen: false,
            dialog: Dialog::None,
            toast: None,
            edit: None,
            last_activity: 0.0,
            last_scan: 0.0,
            load_started: 0.0,
            clipboard: None,
            configured_max_side: false,
            last_title: String::new(),
            scan_rx: None,
            shots: if cfg!(debug_assertions) { std::env::var_os("GLANCE_SHOTS").map(|d| (PathBuf::from(d), 0, 1.5)) } else { None },
        };
        app.open_paths(files);
        app
    }

    // ---------------------------------------------------------------- opening

    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        // Relative paths (e.g. "glance photo.jpg") must become absolute, or the
        // folder can't be listed.
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| p.exists())
            .map(|p| std::path::absolute(&p).unwrap_or(p))
            .collect();
        if paths.is_empty() {
            return;
        }
        self.edit = None;
        if paths.len() == 1 {
            let p = &paths[0];
            if p.is_dir() {
                self.folder = Folder::for_dir(p);
                let first = self.folder.get(0).cloned();
                if first.is_none() {
                    self.show_message("No Images", &format!("There are no images in “{}”.", p.display()));
                }
                self.set_current(first);
            } else {
                self.folder = Folder::for_file(p);
                self.set_current(Some(p.clone()));
            }
        } else {
            let mut files = Vec::new();
            for p in paths {
                if p.is_dir() {
                    files.extend(Folder::for_dir(&p).files);
                } else {
                    files.push(p);
                }
            }
            self.folder = Folder::for_files(files);
            let first = self.folder.get(0).cloned();
            self.set_current(first);
        }
    }

    fn set_current(&mut self, path: Option<PathBuf>) {
        if self.current != path {
            self.tex = None;
            self.frame_idx = 0;
            self.view = self.view.fresh();
        }
        self.current = path;
        self.load_started = f64::NAN; // set on next frame
    }

    fn open_dialog(&mut self) {
        let mut d = rfd::FileDialog::new()
            .set_title("Open Images")
            .add_filter("Images", crate::folder::EXTENSIONS)
            .add_filter("All files", &["*"]);
        if let Some(dir) = self.folder.dir.clone() {
            d = d.set_directory(dir);
        }
        if let Some(files) = d.pick_files() {
            self.open_paths(files);
        }
    }

    fn navigate(&mut self, delta: isize) {
        if self.folder.len() == 0 {
            return;
        }
        let Some(cur) = &self.current else { return };
        let Some(i) = self.folder.index_of(cur) else { return };
        let j = i as isize + delta;
        if j < 0 || j >= self.folder.len() as isize {
            return;
        }
        let next = self.folder.get(j as usize).cloned();
        self.set_current(next);
    }

    fn go_to(&mut self, index: usize) {
        let p = self.folder.get(index).cloned();
        if p.is_some() {
            self.set_current(p);
        }
    }

    fn position(&self) -> Option<(usize, usize)> {
        let cur = self.current.as_ref()?;
        Some((self.folder.index_of(cur)?, self.folder.len()))
    }

    // ---------------------------------------------------------------- actions

    fn show_message(&mut self, title: &str, text: &str) {
        self.dialog = Dialog::Message { title: title.into(), text: text.into() };
    }

    fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.toast = Some((text.into(), now + 3.0));
    }

    fn reload(&mut self) {
        if let Some(p) = self.current.clone() {
            self.loader.forget(&p);
            self.tex = None;
        }
    }

    fn copy_image(&mut self, ctx: &egui::Context) {
        let Some(p) = self.current.clone() else { return };
        let res = crate::loader::decode_full(&p).and_then(|img| {
            let rgba = img.to_rgba8();
            if self.clipboard.is_none() {
                self.clipboard = arboard::Clipboard::new().ok();
            }
            let cb = self.clipboard.as_mut().ok_or("Clipboard unavailable")?;
            cb.set_image(arboard::ImageData {
                width: rgba.width() as usize,
                height: rgba.height() as usize,
                bytes: rgba.into_raw().into(),
            })
            .map_err(|e| e.to_string())
        });
        match res {
            Ok(()) => self.toast(ctx, "Image copied to clipboard"),
            Err(e) => self.show_message("Could Not Copy Image", &e),
        }
    }

    fn after_removal(&mut self, removed: &Path) {
        let idx = self.folder.index_of(removed);
        self.loader.forget(removed);
        self.folder.files.retain(|p| p != removed);
        let next = idx.and_then(|i| self.folder.get(i.min(self.folder.len().saturating_sub(1))).cloned());
        self.tex = None;
        self.current = None;
        self.set_current(next);
    }

    fn trash(&mut self, ctx: &egui::Context) {
        let Some(p) = self.current.clone() else { return };
        match trash::delete(&p) {
            Ok(()) => {
                #[cfg(windows)]
                self.toast(ctx, "Moved to the Recycle Bin");
                #[cfg(not(windows))]
                self.toast(ctx, "Moved to the Trash");
                self.after_removal(&p);
            }
            Err(e) => self.show_message(
                "Could Not Move to Trash",
                &format!("{e}\n\nUse Shift+Delete to delete the file permanently."),
            ),
        }
    }

    fn delete_permanently(&mut self, ctx: &egui::Context) {
        let Some(p) = self.current.clone() else { return };
        match std::fs::remove_file(&p) {
            Ok(()) => {
                self.toast(ctx, "Image deleted");
                self.after_removal(&p);
            }
            Err(e) => self.show_message("Could Not Delete Image", &e.to_string()),
        }
    }

    fn open_with(&mut self) {
        let Some(p) = self.current.clone() else { return };
        if platform::NATIVE_OPEN_WITH {
            if let Err(e) = platform::open_with_dialog(&p) {
                self.show_message("Could Not Open", &e);
            }
        } else {
            let apps = platform::list_image_apps();
            self.dialog = Dialog::OpenWith(apps);
        }
    }

    fn start_edit(&mut self) {
        let Some(t) = &self.tex else { return };
        if t.data.frames.len() > 1 {
            self.show_message("Can't Edit Animations", "Editing animated images isn't supported.");
            return;
        }
        let m = &t.data.meta;
        let mut e = Edit::new([m.width.max(1), m.height.max(1)]);
        // Start the editor with the viewer's rotation applied.
        for _ in 0..self.view.quarter.rem_euclid(4) {
            e.push(Op::RotateCw);
        }
        self.edit = Some(EditSession { edit: e, drag: None });
    }

    fn save_edit(&mut self, ctx: &egui::Context, as_copy: bool) {
        let (Some(p), Some(s)) = (self.current.clone(), self.edit.as_ref()) else { return };
        let e = s.edit.clone();
        let res = crate::loader::decode_full(&p).and_then(|img| {
            let out = e.apply(img);
            let dest = if as_copy { edit::copy_name(&p) } else { p.clone() };
            edit::save(&out, &dest)
        });
        match res {
            Ok(dest) => {
                self.edit = None;
                self.loader.forget(&dest);
                self.folder.rescan();
                if !self.folder.files.contains(&dest) {
                    self.folder.files.push(dest.clone());
                    crate::folder::sort(&mut self.folder.files);
                }
                self.tex = None;
                self.current = None;
                self.set_current(Some(dest.clone()));
                let name = dest.file_name().unwrap_or_default().to_string_lossy().to_string();
                self.toast(ctx, format!("Saved “{name}”"));
            }
            Err(err) => self.show_message("Could Not Save Image", &err),
        }
    }

    // ---------------------------------------------------------------- input

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if !matches!(self.dialog, Dialog::None) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.dialog = Dialog::None;
            }
            return;
        }
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let key = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));

        // Global
        if key(ctrl, Key::Q) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if key(ctrl, Key::W) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if key(ctrl, Key::N) {
            platform::new_window();
        }
        if key(Modifiers::NONE, Key::F1) || key(ctrl, Key::Questionmark) || key(Modifiers::SHIFT, Key::Questionmark) {
            self.dialog = Dialog::Shortcuts;
        }
        // Ctrl+Shift+O must be checked before Ctrl+O (egui ignores extra Shift).
        if key(ctrl_shift, Key::O) {
            if self.edit.is_none() && self.current.is_some() {
                self.open_with();
            }
        } else if key(ctrl, Key::O) {
            self.open_dialog();
        }
        if key(Modifiers::NONE, Key::F11) {
            self.set_fullscreen(ctx, !self.fullscreen);
        }

        if self.edit.is_some() {
            if key(Modifiers::NONE, Key::Escape) {
                self.edit = None;
            } else if key(ctrl_shift, Key::S) {
                self.save_edit(ctx, true);
            } else if key(ctrl, Key::S) || key(ctrl, Key::Enter) {
                self.dialog = Dialog::ConfirmOverwrite;
            } else if key(ctrl_shift, Key::R) {
                self.edit_op(Op::RotateCcw);
            } else if key(ctrl, Key::R) {
                self.edit_op(Op::RotateCw);
            } else if key(Modifiers::NONE, Key::H) {
                self.edit_op(Op::FlipH);
            } else if key(Modifiers::NONE, Key::V) {
                self.edit_op(Op::FlipV);
            }
            return;
        }

        if key(Modifiers::NONE, Key::Escape) && self.fullscreen {
            self.set_fullscreen(ctx, false);
        }
        if self.current.is_none() {
            return;
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos());

        // Panning (must come before plain arrows)
        let step = 60.0;
        if key(ctrl, Key::ArrowLeft) {
            self.view.pan(Vec2::new(step, 0.0));
        }
        if key(ctrl, Key::ArrowRight) {
            self.view.pan(Vec2::new(-step, 0.0));
        }
        if key(ctrl, Key::ArrowUp) {
            self.view.pan(Vec2::new(0.0, step));
        }
        if key(ctrl, Key::ArrowDown) {
            self.view.pan(Vec2::new(0.0, -step));
        }

        let rtl = false;
        if key(Modifiers::NONE, Key::ArrowLeft) || key(Modifiers::NONE, Key::PageUp) {
            self.navigate(if rtl { 1 } else { -1 });
        }
        if key(Modifiers::NONE, Key::ArrowRight) || key(Modifiers::NONE, Key::PageDown) {
            self.navigate(if rtl { -1 } else { 1 });
        }
        if key(Modifiers::NONE, Key::Home) {
            self.go_to(0);
        }
        if key(Modifiers::NONE, Key::End) {
            self.go_to(self.folder.len().saturating_sub(1));
        }

        // Zoom
        if key(ctrl, Key::Plus) || key(ctrl, Key::Equals) || key(Modifiers::NONE, Key::Plus)
            || key(Modifiers::NONE, Key::Equals) || key(Modifiers::SHIFT, Key::Plus)
        {
            self.view.zoom_by(ZOOM_STEP_BUTTON, None);
        }
        if key(ctrl, Key::Minus) || key(Modifiers::NONE, Key::Minus) {
            self.view.zoom_by(1.0 / ZOOM_STEP_BUTTON, None);
        }
        if key(ctrl, Key::Num0) || key(Modifiers::NONE, Key::Num0) {
            self.view.zoom_best_fit();
        }
        if key(ctrl, Key::Num1) || key(Modifiers::NONE, Key::Num1) {
            self.view.zoom_to(1.0, pointer);
        }
        if key(ctrl, Key::Num2) || key(Modifiers::NONE, Key::Num2) {
            self.view.zoom_to(2.0, pointer);
        }

        // Rotate (view only)
        if key(ctrl_shift, Key::R) {
            self.view.rotate(-1);
        }
        if key(ctrl, Key::R) {
            self.view.rotate(1);
        }

        if key(Modifiers::NONE, Key::F9) || key(Modifiers::ALT, Key::Enter) {
            self.show_props = !self.show_props;
        }
        if key(Modifiers::NONE, Key::F5) {
            self.reload();
        }
        if key(ctrl, Key::C) {
            self.copy_image(ctx);
        }
        if key(Modifiers::SHIFT, Key::Delete) {
            self.dialog = Dialog::ConfirmDelete;
        } else if key(Modifiers::NONE, Key::Delete) {
            self.trash(ctx);
        }
        if key(ctrl, Key::F8) {
            self.dialog = Dialog::Wallpaper(WallpaperStyle::Fill);
        }
        if key(ctrl, Key::P) {
            self.request_print(ctx);
        }
        if key(Modifiers::NONE, Key::E) {
            self.start_edit();
        }
        if key(Modifiers::NONE, Key::F10) {
            // Menu is shown from the header; F10 toggles the properties as a fallback.
        }
    }

    fn request_print(&mut self, ctx: &egui::Context) {
        if cfg!(windows) {
            self.do_print(ctx);
        } else {
            self.dialog = Dialog::ConfirmPrint;
        }
    }

    fn do_print(&mut self, ctx: &egui::Context) {
        let Some(p) = self.current.clone() else { return };
        match platform::print(&p) {
            Ok(()) if cfg!(windows) => {}
            Ok(()) => self.toast(ctx, "Sent to the printer"),
            Err(e) => self.show_message("Could Not Print", &e),
        }
    }

    fn edit_op(&mut self, op: Op) {
        if let Some(s) = &mut self.edit {
            s.edit.push(op);
            s.edit.apply_aspect();
        }
    }

    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    // ---------------------------------------------------------------- frame

    fn background_work(&mut self, ctx: &egui::Context) {
        if !self.configured_max_side {
            let max = ctx.input(|i| i.max_texture_side) as u32;
            if max >= 1024 {
                self.loader.max_side = max.min(16384);
                self.configured_max_side = true;
            }
        }
        self.loader.poll();
        let now = ctx.input(|i| i.time);
        if self.load_started.is_nan() {
            self.load_started = now;
        }

        // Watch the folder and current file for changes.
        // The folder listing runs on a background thread: big folders take
        // tens of milliseconds to read and must never stall a frame.
        if let Some(rx) = &self.scan_rx {
            if let Ok(files) = rx.try_recv() {
                self.scan_rx = None;
                self.folder.apply(files);
            }
        }
        if now - self.last_scan > 1.5 && self.edit.is_none() {
            self.last_scan = now;
            if self.scan_rx.is_none() {
                let (dir, explicit) = self.folder.scan_job();
                let (tx, rx) = std::sync::mpsc::channel();
                self.scan_rx = Some(rx);
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::folder::list(dir.as_deref(), explicit));
                    ctx2.request_repaint();
                });
            }
            if let Some(cur) = self.current.clone() {
                if !cur.exists() {
                    self.after_removal(&cur);
                } else if let Some(t) = &self.tex {
                    let mtime = std::fs::metadata(&cur).and_then(|m| m.modified()).ok();
                    if mtime != t.data.mtime {
                        self.reload();
                    }
                }
            }
        }

        let Some(cur) = self.current.clone() else { return };
        self.loader.request(&cur, ctx);
        if let Some((i, n)) = self.position() {
            for j in [i + 1, i.wrapping_sub(1)] {
                if j < n {
                    if let Some(p) = self.folder.get(j).cloned() {
                        self.loader.request(&p, ctx);
                    }
                }
            }
        }
        if self.tex.as_ref().map(|t| t.path != cur).unwrap_or(true) {
            if let Some(Ok(data)) = self.loader.get(&cur) {
                let n = data.frames.len();
                let size = Vec2::new(data.meta.width as f32, data.meta.height as f32);
                let keep_view = self.tex.is_none() && self.view.image_size == size && !self.view.best_fit;
                self.tex = Some(Textures { path: cur.clone(), data, linear: vec![None; n], nearest: vec![None; n] });
                self.frame_idx = 0;
                self.frame_started = now;
                if !keep_view {
                    let q = self.view.quarter;
                    self.view.reset(size);
                    if self.tex.as_ref().map(|t| t.path == cur).unwrap_or(false) && q != 0 {
                        // new image: rotation resets
                    }
                }
            }
        }
        let title = match &self.current {
            Some(p) => format!("{} — Glance", p.file_name().unwrap_or_default().to_string_lossy()),
            None => "Glance".into(),
        };
        if self.last_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }


    /// Scripted walkthrough that saves screenshots (debug builds only).
    fn run_shots(&mut self, ctx: &egui::Context) {
        let Some((dir, step, at)) = self.shots.clone() else { return };
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if now < at {
            return;
        }
        let pick = |s: &Self, name: &str| s.folder.files.iter().find(|p| p.file_name().map(|n| n == name).unwrap_or(false)).cloned();
        let shot = |name: &str| {
            let path = dir.join(name);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                let [w, h] = img.size;
                let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, raw) {
                    let _ = buf.save(&path);
                }
            })));
        };
        // Even ticks set up a state, odd ticks photograph it.
        const NAMES: [&str; 12] = [
            "01-viewer", "02-properties", "03-transparent", "04-zoom-200", "05-svg", "06-error",
            "07-edit", "08-wallpaper-dialog", "09-shortcuts", "10-light", "11-gif", "12-empty",
        ];
        let k = step / 2;
        if k >= NAMES.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if step % 2 == 1 {
            shot(&format!("{}.png", NAMES[k]));
        } else {
            self.dialog = Dialog::None;
            self.edit = None;
            self.show_props = false;
            ctx.set_theme(egui::Theme::Dark);
            match k {
                0 => {}
                1 => self.show_props = true,
                2 => { let p = pick(self, "photo 2.png"); self.set_current(p); }
                3 => self.view.zoom_to(2.0, None),
                4 => { let p = pick(self, "vector.svg"); self.set_current(p); }
                5 => { let p = pick(self, "broken.png"); self.set_current(p); }
                6 => {
                    let p = pick(self, "photo 1.jpg");
                    self.set_current(p);
                    if self.tex.is_some() {
                        self.start_edit();
                        if let Some(e) = &mut self.edit { e.edit.aspect = Aspect::Square; e.edit.apply_aspect(); }
                    }
                }
                7 => self.dialog = Dialog::Wallpaper(WallpaperStyle::Fill),
                8 => self.dialog = Dialog::Shortcuts,
                9 => { ctx.set_theme(egui::Theme::Light); self.show_props = true; }
                10 => { let p = pick(self, "photo 3.gif"); self.set_current(p); }
                _ => { self.current = None; self.tex = None; }
            }
        }
        if step % 2 == 0 && k == 6 {
            // Editing needs the image loaded: retry until it is.
            if self.edit.is_none() {
                self.shots = Some((dir, step, now + 0.3));
                return;
            }
        }
        self.shots = Some((dir, step + 1, now + 1.0));
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.open_paths(dropped);
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("header")
            .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(egui::Margin::symmetric(8, 6)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.edit.is_some() {
                        if ui.button("Cancel").clicked() {
                            self.edit = None;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let save = egui::Button::new(RichText::new("Save").strong())
                                .fill(ui.visuals().selection.bg_fill);
                            let r = ui.add(save).on_hover_text("Replace the original (Ctrl+S)");
                            if r.clicked() {
                                self.dialog = Dialog::ConfirmOverwrite;
                            }
                            if ui.button("Save as Copy").on_hover_text("Ctrl+Shift+S").clicked() {
                                self.save_edit(&ctx, true);
                            }
                            ui.centered_and_justified(|ui| ui.strong("Edit Image"));
                        });
                        return;
                    }
                    if icons::labeled(ui, Icon::Open, "Open…", "Open images (Ctrl+O)").clicked() {
                        self.open_dialog();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.main_menu(ui, &ctx);
                        let has = self.current.is_some();
                        if icons::button_enabled(ui, Icon::Info, "Image properties (F9)", self.show_props, has).clicked()
                        {
                            self.show_props = !self.show_props;
                        }
                        if icons::button_enabled(ui, Icon::Edit, "Edit: crop, rotate and flip (E)", false, has).clicked() {
                            self.start_edit();
                        }
                        ui.centered_and_justified(|ui| {
                            let name = self
                                .current
                                .as_ref()
                                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                                .unwrap_or_else(|| "Glance".into());
                            let mut label = RichText::new(name).strong();
                            if let Some((i, n)) = self.position() {
                                if n > 1 {
                                    label = RichText::new(format!(
                                        "{}   {}/{}",
                                        self.current.as_ref().unwrap().file_name().unwrap_or_default().to_string_lossy(),
                                        i + 1,
                                        n
                                    ))
                                    .strong();
                                }
                            }
                            ui.add(egui::Label::new(label).truncate());
                        });
                    });
                });
            });
    }

    fn main_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has = self.current.is_some();
        let menu_resp = icons::button(ui, Icon::Menu, "Main menu", false);
        egui::Popup::menu(&menu_resp).show(|ui| {
            ui.set_min_width(230.0);
            if ui.add(egui::Button::new("New Window").shortcut_text("Ctrl+N")).clicked() {
                platform::new_window();
            }
            ui.separator();
            if ui.add_enabled(has, egui::Button::new("Open With…").shortcut_text("Ctrl+Shift+O")).clicked() {
                self.open_with();
            }
            #[cfg(windows)]
            let show_label = "Show in File Explorer";
            #[cfg(not(windows))]
            let show_label = "Show in Folder";
            if ui.add_enabled(has, egui::Button::new(show_label)).clicked() {
                if let Some(p) = &self.current {
                    if let Err(e) = platform::show_in_folder(p) {
                        self.show_message("Could Not Open Folder", &e);
                    }
                }
            }
            if ui.add_enabled(has, egui::Button::new("Print…").shortcut_text("Ctrl+P")).clicked() {
                self.request_print(ctx);
            }
            if ui.add_enabled(has, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                self.copy_image(ctx);
            }
            #[cfg(windows)]
            let trash_label = "Move to Recycle Bin";
            #[cfg(not(windows))]
            let trash_label = "Move to Trash";
            if ui.add_enabled(has, egui::Button::new(trash_label).shortcut_text("Delete")).clicked() {
                self.trash(ctx);
            }
            if ui.add_enabled(has, egui::Button::new("Delete Permanently…").shortcut_text("Shift+Delete")).clicked() {
                self.dialog = Dialog::ConfirmDelete;
            }
            ui.separator();
            if ui.add_enabled(has, egui::Button::new("Rotate Clockwise").shortcut_text("Ctrl+R")).clicked() {
                self.view.rotate(1);
            }
            if ui.add_enabled(has, egui::Button::new("Rotate Counter-Clockwise").shortcut_text("Ctrl+Shift+R")).clicked() {
                self.view.rotate(-1);
            }
            if ui.add_enabled(has, egui::Button::new("Set as Background…").shortcut_text("Ctrl+F8")).clicked() {
                self.dialog = Dialog::Wallpaper(WallpaperStyle::Fill);
            }
            if ui.add_enabled(has, egui::Button::new("Reload").shortcut_text("F5")).clicked() {
                self.reload();
            }
            ui.separator();
            if ui.add(egui::Button::new("Fullscreen").shortcut_text("F11")).clicked() {
                self.set_fullscreen(ctx, !self.fullscreen);
            }
            if ui.add(egui::Button::new("Keyboard Shortcuts").shortcut_text("F1")).clicked() {
                self.dialog = Dialog::Shortcuts;
            }
            if ui.button("About Glance").clicked() {
                self.dialog = Dialog::About;
            }
        });
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        let Some(t) = &self.tex else { return };
        let m = t.data.meta.clone();
        let path = t.path.clone();
        egui::Panel::right("properties")
            .resizable(true)
            .default_size(290.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(8.0);
                    let row = |ui: &mut egui::Ui, k: &str, v: Option<String>| {
                        if let Some(v) = v {
                            ui.label(RichText::new(k).small().weak());
                            ui.add(egui::Label::new(v).wrap());
                            ui.add_space(6.0);
                        }
                    };
                    let section = |ui: &mut egui::Ui, title: &str| {
                        ui.add_space(4.0);
                        ui.heading(RichText::new(title).size(15.0).strong());
                        ui.add_space(4.0);
                    };

                    section(ui, "Image");
                    let mut dims = format!("{} × {} pixels", m.width, m.height);
                    if t.data.downscaled {
                        dims.push_str(" (shown reduced)");
                    }
                    row(ui, "Dimensions", Some(dims));
                    row(ui, "Format", Some(m.format.clone()));
                    let color = m.color.clone().map(|c| match m.bit_depth {
                        Some(d) => format!("{c}, {d}-bit"),
                        None => c,
                    });
                    row(ui, "Color", color);
                    row(ui, "Transparency", m.alpha.map(|a| if a { "Yes" } else { "No" }.to_string()));
                    if m.frames > 1 {
                        row(ui, "Animation", Some(format!("{} frames", m.frames)));
                    }

                    ui.separator();
                    section(ui, "File");
                    row(ui, "Name", Some(m.file_name.clone()));
                    if let Some(folder) = &m.folder {
                        ui.label(RichText::new("Folder").small().weak());
                        if ui.link(folder).on_hover_text("Show in folder").clicked() {
                            let _ = platform::show_in_folder(&path);
                        }
                        ui.add_space(6.0);
                    }
                    row(ui, "Size", m.file_size.map(meta::human_size));
                    row(ui, "Created", m.created.clone());
                    row(ui, "Modified", m.modified.clone());

                    let e = &m.exif;
                    let any_exif = e.date_taken.is_some() || e.camera.is_some() || e.exposure.is_some()
                        || e.creator.is_some() || e.copyright.is_some() || e.software.is_some() || e.comment.is_some();
                    if any_exif {
                        ui.separator();
                        section(ui, "Details");
                        row(ui, "Date Taken", e.date_taken.clone());
                        row(ui, "Camera", e.camera.clone());
                        row(ui, "Lens", e.lens.clone());
                        let exposure: Vec<String> = [&e.f_number, &e.exposure, &e.iso, &e.focal_length]
                            .iter()
                            .filter_map(|x| (*x).clone())
                            .collect();
                        row(ui, "Exposure", (!exposure.is_empty()).then(|| exposure.join("  ·  ")));
                        row(ui, "Description", e.comment.clone());
                        row(ui, "Creator", e.creator.clone());
                        row(ui, "Copyright", e.copyright.clone());
                        row(ui, "Software", e.software.clone());
                    }
                    if let Some(gps) = e.gps {
                        ui.separator();
                        section(ui, "Location");
                        ui.label(meta::format_coords(gps));
                        if let Some(a) = &e.altitude {
                            ui.label(RichText::new(format!("Altitude {a}")).weak());
                        }
                        if ui.link("Open in OpenStreetMap").clicked() {
                            platform::open_url(&meta::map_url(gps));
                        }
                    }
                });
            });
    }

    fn image_area(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let dark = ui.visuals().dark_mode;
        let bg = if dark { Color32::from_gray(24) } else { Color32::from_gray(236) };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(bg))
            .show(ui, |ui| {
                let rect = ui.max_rect();
                let resp = ui.interact(rect, Id::unique("image-area"), Sense::click_and_drag());
                let now = ctx.input(|i| i.time);
                let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_pressed());
                if moved {
                    self.last_activity = now;
                }

                let Some(cur) = self.current.clone() else {
                    self.empty_state(ui, rect);
                    return;
                };

                if self.edit.is_some() {
                    self.editor(ui, rect);
                    return;
                }

                // Error?
                if let Some(Err(e)) = self.loader.get(&cur) {
                    self.error_state(ui, rect, &cur, &e);
                    self.nav_overlay(ui, rect, now);
                    return;
                }

                let ppp = ctx.pixels_per_point();
                let dt = ctx.input(|i| i.stable_dt).min(0.1);
                self.view.update(rect, ppp, dt);
                if std::env::var_os("GLANCE_DEBUG").is_some() {
                    eprintln!(
                        "t={now:.2} vp={:?} ppp={ppp} zoom={:.3} target={:.3} fit={:.3} img={:?} best_fit={} tex={}",
                        rect, self.view.zoom, self.view.target_zoom, self.view.fit_zoom(), self.view.image_size,
                        self.view.best_fit, self.tex.is_some()
                    );
                }

                // Mouse/touchpad input on the image
                if resp.dragged_by(PointerButton::Primary) || resp.dragged_by(PointerButton::Middle) {
                    self.view.pan(resp.drag_delta());
                }
                if resp.drag_stopped() {
                    // Keep gliding after release, like Loupe's kinetic panning.
                    let v = ctx.input(|i| i.pointer.velocity());
                    self.view.fling(v);
                }
                if resp.double_clicked() {
                    self.set_fullscreen(&ctx, !self.fullscreen);
                }
                if resp.hovered() {
                    let (events, zoom_delta, pos) = ctx.input(|i| (i.events.clone(), i.zoom_delta(), i.pointer.hover_pos()));
                    for ev in events {
                        if let egui::Event::MouseWheel { unit, delta, modifiers, .. } = ev {
                            let discrete = !matches!(unit, egui::MouseWheelUnit::Point);
                            if discrete || modifiers.command {
                                // Wheel: zoom like a magnifier around the cursor.
                                let steps = if discrete { delta.y } else { delta.y / 50.0 };
                                if steps != 0.0 {
                                    self.view.zoom_by(ZOOM_STEP_WHEEL.powf(steps.clamp(-3.0, 3.0)), pos);
                                }
                            } else if self.view.can_pan() {
                                self.view.pan(delta);
                            } else if delta.x.abs() > 30.0 && delta.x.abs() > delta.y.abs() {
                                self.navigate(if delta.x < 0.0 { 1 } else { -1 });
                            }
                        }
                    }
                    if zoom_delta != 1.0 {
                        self.view.zoom_by(zoom_delta, pos);
                    }
                }
                if ctx.input(|i| i.pointer.button_pressed(PointerButton::Extra1)) {
                    self.navigate(-1);
                }
                if ctx.input(|i| i.pointer.button_pressed(PointerButton::Extra2)) {
                    self.navigate(1);
                }
                if self.view.can_pan() && resp.hovered() {
                    ctx.set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                }

                let painter = ui.painter_at(rect);
                if let Some(t) = &mut self.tex {
                    let n = t.data.frames.len();
                    // Animation
                    if n > 1 {
                        let delay = t.data.frames[self.frame_idx].delay.as_secs_f64();
                        if now - self.frame_started >= delay {
                            self.frame_idx = (self.frame_idx + 1) % n;
                            self.frame_started = now;
                        }
                        let remain = (self.frame_started + t.data.frames[self.frame_idx].delay.as_secs_f64() - now).max(0.0);
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(remain));
                    }
                    // Crisp pixels when zoomed in far.
                    let disp = t.data.display_size();
                    let tex_scale = self.view.zoom * self.view.image_size.x / disp[0] as f32;
                    let nearest = tex_scale >= 3.0 && !t.data.is_svg;
                    let id = t.get(&ctx, self.frame_idx, nearest);
                    let checker = t.data.meta.alpha.unwrap_or(false);
                    self.view.draw(&painter, id, checker, dark);
                    if self.view.is_animating() {
                        ctx.request_repaint();
                    }
                } else if self.loader.is_pending(&cur) && now - self.load_started > 0.25 {
                    let s = 32.0;
                    ui.put(Rect::from_center_size(rect.center(), Vec2::splat(s)), egui::Spinner::new().size(s));
                }

                // Drag-and-drop hint
                if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
                    drop_overlay(&painter, rect);
                }

                self.nav_overlay(ui, rect, now);
                self.zoom_overlay(ui, rect, now);
            });
    }

    fn controls_visible(&self, ctx: &egui::Context, now: f64) -> bool {
        let idle = now - self.last_activity;
        let hover = ctx.input(|i| i.pointer.has_pointer());
        hover && idle < 2.5
    }

    fn nav_overlay(&mut self, ui: &mut egui::Ui, rect: Rect, now: f64) {
        let ctx = ui.ctx().clone();
        let Some((i, n)) = self.position() else { return };
        if n < 2 {
            return;
        }
        let visible = self.controls_visible(&ctx, now);
        let alpha = ctx.animate_bool_with_time(Id::unique("nav-vis"), visible, 0.2);
        if alpha <= 0.0 {
            return;
        }
        if visible {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        let size = 44.0;
        let button = |ui: &mut egui::Ui, center: Pos2, label: &str, icon: Icon, enabled: bool| -> bool {
            let r = Rect::from_center_size(center, Vec2::splat(size));
            let resp = ui.interact(r, Id::unique(("nav", label)), Sense::click());
            let fill = if resp.hovered() && enabled { 200 } else { 150 };
            let a = (alpha * if enabled { 1.0 } else { 0.35 }).clamp(0.0, 1.0);
            let p = ui.painter();
            p.circle_filled(center, size / 2.0, Color32::from_black_alpha((fill as f32 * a) as u8));
            icons::paint(p, Rect::from_center_size(center, Vec2::splat(22.0)), icon, Color32::WHITE.gamma_multiply(a));
            enabled && resp.clicked()
        };
        let y = rect.center().y;
        if button(ui, Pos2::new(rect.left() + 18.0 + size / 2.0, y), "prev", Icon::Prev, i > 0) {
            self.navigate(-1);
        }
        if button(ui, Pos2::new(rect.right() - 18.0 - size / 2.0, y), "next", Icon::Next, i + 1 < n) {
            self.navigate(1);
        }
    }

    fn zoom_overlay(&mut self, ui: &mut egui::Ui, rect: Rect, now: f64) {
        let ctx = ui.ctx().clone();
        let visible = self.controls_visible(&ctx, now);
        let alpha = ctx.animate_bool_with_time(Id::unique("zoom-vis"), visible, 0.2);
        if alpha <= 0.0 {
            return;
        }
        let pct = (self.view.target_zoom * 100.0).round();
        egui::Area::new(Id::unique("zoom-controls"))
            .fixed_pos(rect.right_bottom() - Vec2::new(18.0, 18.0))
            .pivot(Align2::RIGHT_BOTTOM)
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                ui.set_opacity(alpha);
                egui::Frame::popup(&ctx.global_style()).corner_radius(CornerRadius::same(20)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if icons::button(ui, Icon::RotateLeft, "Rotate counter-clockwise (Ctrl+Shift+R)", false).clicked() {
                            self.view.rotate(-1);
                        }
                        if icons::button(ui, Icon::RotateRight, "Rotate clockwise (Ctrl+R)", false).clicked() {
                            self.view.rotate(1);
                        }
                        ui.separator();
                        if icons::button(ui, Icon::ZoomOut, "Zoom out (Ctrl+−)", false).clicked() {
                            self.view.zoom_by(1.0 / ZOOM_STEP_BUTTON, None);
                        }
                        ui.menu_button(format!("{pct}%"), |ui| {
                            if ui.button("Best Fit").clicked() {
                                self.view.zoom_best_fit();
                            }
                            for z in [0.5, 1.0, 2.0, 3.0] {
                                if ui.button(format!("{}%", (z * 100.0) as i32)).clicked() {
                                    self.view.zoom_to(z, None);
                                }
                            }
                        });
                        if icons::button(ui, Icon::ZoomIn, "Zoom in (Ctrl++)", false).clicked() {
                            self.view.zoom_by(ZOOM_STEP_BUTTON, None);
                        }
                        let fit_label = if self.view.best_fit { "1:1" } else { "Fit" };
                        if ui.button(fit_label).on_hover_text("Toggle best fit / 100% (Ctrl+0 / Ctrl+1)").clicked() {
                            if self.view.best_fit {
                                self.view.zoom_to(1.0, None);
                            } else {
                                self.view.zoom_best_fit();
                            }
                        }
                    });
                });
            });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)));
        child.add_space((rect.height() / 2.0 - 120.0).max(10.0));
        let tex_id = {
            let icon = crate::icon::render(128);
            let img = egui::ColorImage::from_rgba_unmultiplied([128, 128], icon.as_raw());
            let h = child.ctx().load_texture("app-icon", img, TextureOptions::LINEAR);
            let id = h.id();
            child.data_mut(|d| d.insert_temp(Id::unique("icon-handle"), h));
            id
        };
        child.image(egui::load::SizedTexture::new(tex_id, Vec2::splat(96.0)));
        child.add_space(12.0);
        child.heading(RichText::new("View Images").size(24.0).strong());
        child.label(RichText::new("Drag and drop images here").weak());
        child.add_space(14.0);
        if child.add(egui::Button::new(RichText::new("Open Files…").size(15.0)).min_size(Vec2::new(140.0, 32.0))).clicked() {
            self.open_dialog();
        }
        if ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
            drop_overlay(ui.painter(), rect);
        }
    }

    fn error_state(&mut self, ui: &mut egui::Ui, rect: Rect, path: &Path, err: &str) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)));
        child.add_space((rect.height() / 2.0 - 90.0).max(10.0));
        let (r, _) = child.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
        icons::paint(child.painter(), r, Icon::Warning, child.visuals().warn_fg_color);
        let title = if crate::loader::looks_like_image(path) { "Could Not Load Image" } else { "Unknown Image Format" };
        child.heading(RichText::new(title).strong());
        child.add_space(6.0);
        child.add(egui::Label::new(RichText::new(err).weak()).wrap());
        child.add_space(10.0);
        if child.button("Try Again").clicked() {
            self.reload();
        }
    }

    // ---------------------------------------------------------------- editor

    fn editor(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let dark = ui.visuals().dark_mode;
        // Toolbar at the bottom.
        let bar_h = 52.0;
        let bar = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - bar_h), rect.right_bottom());
        let area = Rect::from_min_max(rect.min, Pos2::new(rect.right(), bar.top())).shrink(28.0);

        let Some(t) = &mut self.tex else { return };
        let tex_id = t.get(&ctx, 0, false);
        let alpha = t.data.meta.alpha.unwrap_or(false);
        let Some(sess) = &mut self.edit else { return };
        let e = &mut sess.edit;

        let [tw, th] = e.transformed_size();
        let scale = (area.width() / tw as f32).min(area.height() / th as f32);
        let img_rect = Rect::from_center_size(area.center(), Vec2::new(tw as f32, th as f32) * scale);
        let painter = ui.painter_at(rect);
        if alpha {
            view::draw_checkerboard(&painter, img_rect, egui::emath::Rot2::IDENTITY, img_rect.center(), dark);
        }
        view::draw_with_corners(&painter, tex_id, img_rect, edit::uv_corners(&e.ops));

        let to_screen = |p: Pos2| img_rect.min + (p.to_vec2() * img_rect.size());
        let crop = Rect::from_min_max(to_screen(e.crop.min), to_screen(e.crop.max));

        // Dim outside the crop
        let shade = Color32::from_black_alpha(150);
        for r in [
            Rect::from_min_max(img_rect.min, Pos2::new(img_rect.max.x, crop.min.y)),
            Rect::from_min_max(Pos2::new(img_rect.min.x, crop.max.y), img_rect.max),
            Rect::from_min_max(Pos2::new(img_rect.min.x, crop.min.y), Pos2::new(crop.min.x, crop.max.y)),
            Rect::from_min_max(Pos2::new(crop.max.x, crop.min.y), Pos2::new(img_rect.max.x, crop.max.y)),
        ] {
            if r.is_positive() {
                painter.rect_filled(r, 0.0, shade);
            }
        }
        // Thirds + border + handles
        let thin = Stroke::new(1.0, Color32::from_white_alpha(90));
        for k in 1..3 {
            let f = k as f32 / 3.0;
            let x = crop.min.x + crop.width() * f;
            let y = crop.min.y + crop.height() * f;
            painter.line_segment([Pos2::new(x, crop.min.y), Pos2::new(x, crop.max.y)], thin);
            painter.line_segment([Pos2::new(crop.min.x, y), Pos2::new(crop.max.x, y)], thin);
        }
        painter.rect_stroke(crop, 0.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
        let hs = 14.0;
        for c in [crop.left_top(), crop.right_top(), crop.left_bottom(), crop.right_bottom()] {
            painter.circle_filled(c, hs / 2.0, Color32::WHITE);
        }

        // Interaction
        let resp = ui.interact(img_rect.expand(hs), Id::unique("crop"), Sense::drag());
        let hit = |p: Pos2| -> Option<Handle> {
            let near = |a: f32, b: f32| (a - b).abs() <= hs;
            let (l, r, tp, b) = (near(p.x, crop.left()), near(p.x, crop.right()), near(p.y, crop.top()), near(p.y, crop.bottom()));
            let inx = p.x > crop.left() - hs && p.x < crop.right() + hs;
            let iny = p.y > crop.top() - hs && p.y < crop.bottom() + hs;
            Some(match (l, r, tp, b) {
                (true, _, true, _) => Handle::NW,
                (_, true, true, _) => Handle::NE,
                (true, _, _, true) => Handle::SW,
                (_, true, _, true) => Handle::SE,
                (true, _, _, _) if iny => Handle::W,
                (_, true, _, _) if iny => Handle::E,
                (_, _, true, _) if inx => Handle::N,
                (_, _, _, true) if inx => Handle::S,
                _ if crop.contains(p) => Handle::Move,
                _ => return None,
            })
        };
        if let Some(p) = resp.hover_pos() {
            let icon = match sess.drag.map(|d| d.0).or_else(|| hit(p)) {
                Some(Handle::Move) => egui::CursorIcon::Move,
                Some(Handle::N) | Some(Handle::S) => egui::CursorIcon::ResizeVertical,
                Some(Handle::E) | Some(Handle::W) => egui::CursorIcon::ResizeHorizontal,
                Some(Handle::NW) | Some(Handle::SE) => egui::CursorIcon::ResizeNwSe,
                Some(Handle::NE) | Some(Handle::SW) => egui::CursorIcon::ResizeNeSw,
                None => egui::CursorIcon::Default,
            };
            ctx.set_cursor_icon(icon);
        }
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                sess.drag = hit(p).map(|h| (h, e.crop));
            }
        }
        if resp.drag_stopped() {
            sess.drag = None;
        }
        if let (Some((h, start)), true) = (sess.drag, resp.dragged()) {
            let total = resp.total_drag_delta().unwrap_or_default() / img_rect.size();
            let min_w = (8.0 / tw as f32).max(0.02);
            let min_h = (8.0 / th as f32).max(0.02);
            let r;
            match h {
                Handle::Move => {
                    let d = egui::vec2(
                        total.x.clamp(-start.min.x, 1.0 - start.max.x),
                        total.y.clamp(-start.min.y, 1.0 - start.max.y),
                    );
                    r = start.translate(d);
                }
                _ => {
                    let (mut l, mut tp, mut rr, mut b) = (start.min.x, start.min.y, start.max.x, start.max.y);
                    if matches!(h, Handle::W | Handle::NW | Handle::SW) { l = (l + total.x).clamp(0.0, rr - min_w); }
                    if matches!(h, Handle::E | Handle::NE | Handle::SE) { rr = (rr + total.x).clamp(l + min_w, 1.0); }
                    if matches!(h, Handle::N | Handle::NW | Handle::NE) { tp = (tp + total.y).clamp(0.0, b - min_h); }
                    if matches!(h, Handle::S | Handle::SW | Handle::SE) { b = (b + total.y).clamp(tp + min_h, 1.0); }
                    if let Some(ratio) = e.ratio() {
                        // Keep the aspect ratio: derive height from width
                        // (or width from height for N/S handles).
                        let ar = ratio * th as f32 / tw as f32; // ratio in normalized units
                        if matches!(h, Handle::N | Handle::S) {
                            let w = (b - tp) * ar;
                            let cx = (l + rr) / 2.0;
                            l = cx - w / 2.0;
                            rr = cx + w / 2.0;
                        } else {
                            let hgt = (rr - l) / ar;
                            if matches!(h, Handle::N | Handle::NW | Handle::NE) { tp = b - hgt; } else if matches!(h, Handle::E | Handle::W) {
                                let cy = (tp + b) / 2.0;
                                tp = cy - hgt / 2.0;
                                b = cy + hgt / 2.0;
                            } else { b = tp + hgt; }
                        }
                        if l >= 0.0 && tp >= 0.0 && rr <= 1.0 && b <= 1.0 {
                            r = Rect::from_min_max(Pos2::new(l, tp), Pos2::new(rr, b));
                        } else {
                            r = e.crop; // would leave the image: keep last valid
                        }
                    } else {
                        r = Rect::from_min_max(Pos2::new(l, tp), Pos2::new(rr, b));
                    }
                }
            }
            e.crop = r;
        }

        // Toolbar
        let mut op = None;
        let mut reset = false;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(12.0, 8.0))).layout(Layout::left_to_right(Align::Center)));
        if icons::labeled(&mut child, Icon::RotateLeft, "Rotate Left", "Ctrl+Shift+R").clicked() { op = Some(Op::RotateCcw); }
        if icons::labeled(&mut child, Icon::RotateRight, "Rotate Right", "Ctrl+R").clicked() { op = Some(Op::RotateCw); }
        if icons::labeled(&mut child, Icon::FlipH, "Flip", "Flip horizontally (H)").clicked() { op = Some(Op::FlipH); }
        if icons::labeled(&mut child, Icon::FlipV, "Flip", "Flip vertically (V)").clicked() { op = Some(Op::FlipV); }
        child.separator();
        child.label("Aspect:");
        let current = Aspect::PRESETS.iter().find(|(_, a)| *a == e.aspect).map(|(n, _)| *n).unwrap_or("Free");
        let mut new_aspect = None;
        egui::ComboBox::from_id_salt("aspect").selected_text(current).show_ui(&mut child, |ui| {
            for (name, a) in Aspect::PRESETS {
                if ui.selectable_label(e.aspect == a, name).clicked() {
                    new_aspect = Some(a);
                }
            }
        });
        if matches!(e.aspect, Aspect::Ratio(..)) {
            let label = if e.portrait { "Portrait" } else { "Landscape" };
            if child.button(label).on_hover_text("Swap orientation").clicked() {
                e.portrait = !e.portrait;
                e.apply_aspect();
            }
        }
        child.separator();
        if child.add_enabled(e.is_modified(), egui::Button::new("Reset")).clicked() { reset = true; }
        let [cw, ch] = [
            (e.crop.width() * tw as f32).round() as u32,
            (e.crop.height() * th as f32).round() as u32,
        ];
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("{cw} × {ch}")).weak());
        });

        if let Some(a) = new_aspect {
            e.aspect = a;
            e.portrait = tw < th;
            e.apply_aspect();
        }
        if reset {
            *e = Edit::new(e.src_size);
        }
        if let Some(op) = op {
            e.push(op);
            e.apply_aspect();
        }
    }

    // ---------------------------------------------------------------- dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut action: Option<Box<dyn FnOnce(&mut Self, &egui::Context)>> = None;
        let name = self
            .current
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let modal = |id: &str, ctx: &egui::Context, f: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Modal::new(Id::unique(id)).show(ctx, |ui| {
                ui.set_max_width(420.0);
                f(ui)
            })
        };

        match &mut self.dialog {
            Dialog::None => {}
            Dialog::ConfirmDelete => {
                let r = modal("delete", ctx, &mut |ui| {
                    ui.heading("Delete Permanently?");
                    ui.add_space(6.0);
                    ui.label(format!("“{name}” will be deleted and can't be restored."));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { close = true; }
                        let del = egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(Color32::from_rgb(192, 28, 40));
                        if ui.add(del).clicked() {
                            close = true;
                            action = Some(Box::new(|s, c| s.delete_permanently(c)));
                        }
                    });
                });
                if r.should_close() { close = true; }
            }
            Dialog::Wallpaper(style) => {
                let mut style_v = *style;
                let r = modal("wallpaper", ctx, &mut |ui| {
                    ui.heading("Set as Desktop Background");
                    ui.add_space(6.0);
                    ui.label(format!("Use “{name}” as your desktop background."));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Fit:");
                        egui::ComboBox::from_id_salt("wp-style").selected_text(style_v.label()).show_ui(ui, |ui| {
                            for s in WallpaperStyle::ALL {
                                ui.selectable_value(&mut style_v, s, s.label());
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { close = true; }
                        let b = egui::Button::new(RichText::new("Set Background").strong()).fill(ui.visuals().selection.bg_fill);
                        if ui.add(b).clicked() {
                            close = true;
                            let st = style_v;
                            action = Some(Box::new(move |s, c| {
                                if let Some(p) = s.current.clone() {
                                    match platform::set_wallpaper(&p, st) {
                                        Ok(msg) => s.toast(c, msg),
                                        Err(e) => s.show_message("Could Not Set Background", &e),
                                    }
                                }
                            }));
                        }
                    });
                });
                *style = style_v;
                if r.should_close() { close = true; }
            }
            Dialog::ConfirmOverwrite => {
                let r = modal("overwrite", ctx, &mut |ui| {
                    ui.heading("Replace the Original?");
                    ui.add_space(6.0);
                    ui.label(format!("“{name}” will be overwritten with the edited image. Camera details and other metadata are not kept."));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { close = true; }
                        if ui.button("Save as Copy").clicked() {
                            close = true;
                            action = Some(Box::new(|s, c| s.save_edit(c, true)));
                        }
                        let b = egui::Button::new(RichText::new("Replace").strong()).fill(ui.visuals().selection.bg_fill);
                        if ui.add(b).clicked() {
                            close = true;
                            action = Some(Box::new(|s, c| s.save_edit(c, false)));
                        }
                    });
                });
                if r.should_close() { close = true; }
            }
            Dialog::ConfirmPrint => {
                let r = modal("print", ctx, &mut |ui| {
                    ui.heading("Print Image?");
                    ui.add_space(6.0);
                    ui.label(format!("“{name}” will be sent to your default printer, scaled to fit the page."));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { close = true; }
                        if ui.button("Print").clicked() {
                            close = true;
                            action = Some(Box::new(|s, c| s.do_print(c)));
                        }
                    });
                });
                if r.should_close() { close = true; }
            }
            Dialog::OpenWith(apps) => {
                let apps = apps.clone();
                let r = modal("openwith", ctx, &mut |ui| {
                    ui.heading("Open With");
                    ui.add_space(6.0);
                    if apps.is_empty() {
                        ui.label("No other apps that open images were found.");
                    }
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for app in &apps {
                            if ui.add(egui::Button::new(&app.name).min_size(Vec2::new(360.0, 28.0))).clicked() {
                                close = true;
                                let app = app.clone();
                                action = Some(Box::new(move |s, _| {
                                    if let Some(p) = s.current.clone() {
                                        if let Err(e) = platform::launch_app(&app, &p) {
                                            s.show_message("Could Not Open", &e);
                                        }
                                    }
                                }));
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { close = true; }
                        if ui.button("Default App").clicked() {
                            close = true;
                            action = Some(Box::new(|s, _| {
                                if let Some(p) = s.current.clone() {
                                    let _ = platform::open_with_dialog(&p);
                                }
                            }));
                        }
                    });
                });
                if r.should_close() { close = true; }
            }
            Dialog::Shortcuts => {
                let r = modal("shortcuts", ctx, &mut |ui| {
                    ui.heading("Keyboard Shortcuts");
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                        for (section, items) in SHORTCUTS {
                            ui.add_space(6.0);
                            ui.strong(*section);
                            egui::Grid::new(*section).num_columns(2).min_col_width(200.0).spacing([24.0, 4.0]).show(ui, |ui| {
                                for (what, keys) in *items {
                                    ui.label(*what);
                                    ui.label(RichText::new(*keys).monospace().weak());
                                    ui.end_row();
                                }
                            });
                        }
                    });
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() { close = true; }
                });
                if r.should_close() { close = true; }
            }
            Dialog::About => {
                let r = modal("about", ctx, &mut |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading(RichText::new("Glance").size(22.0).strong());
                        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                        ui.add_space(6.0);
                        ui.label("A fast, simple image viewer for Windows and Linux.");
                        ui.label(RichText::new("Inspired by GNOME's Loupe image viewer. Not affiliated with GNOME.").small().weak());
                        ui.add_space(10.0);
                        if ui.button("Close").clicked() { close = true; }
                    });
                });
                if r.should_close() { close = true; }
            }
            Dialog::Message { title, text } => {
                let (title, text) = (title.clone(), text.clone());
                let r = modal("message", ctx, &mut |ui| {
                    ui.heading(&title);
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(&text).wrap());
                    ui.add_space(12.0);
                    if ui.button("OK").clicked() { close = true; }
                });
                if r.should_close() { close = true; }
            }
        }
        if close {
            self.dialog = Dialog::None;
        }
        if let Some(a) = action {
            a(self, ctx);
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((text, until)) = self.toast.clone() else { return };
        let now = ctx.input(|i| i.time);
        if now > until {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        let screen = ctx.content_rect();
        egui::Area::new(Id::unique("toast"))
            .fixed_pos(Pos2::new(screen.center().x, screen.bottom() - 80.0))
            .pivot(Align2::CENTER_BOTTOM)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::popup(&ctx.global_style()).corner_radius(CornerRadius::same(18)).inner_margin(egui::Margin::symmetric(16, 8)).show(ui, |ui| {
                    ui.label(&text);
                });
            });
    }
}

fn drop_overlay(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect.shrink(12.0), 16.0, Color32::from_rgba_unmultiplied(53, 132, 228, 60));
    painter.rect_stroke(rect.shrink(12.0), 16.0, Stroke::new(3.0, Color32::from_rgb(53, 132, 228)), egui::StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, "Drop to open", egui::FontId::proportional(22.0), Color32::WHITE);
}

const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    ("General", &[
        ("Open images", "Ctrl+O"),
        ("New window", "Ctrl+N"),
        ("Close window", "Ctrl+W"),
        ("Quit", "Ctrl+Q"),
        ("Keyboard shortcuts", "F1"),
        ("Fullscreen", "F11 / double-click"),
        ("Leave fullscreen", "Esc"),
        ("Image properties", "F9 / Alt+Enter"),
    ]),
    ("Browsing", &[
        ("Next image", "Right / Page Down / Mouse forward"),
        ("Previous image", "Left / Page Up / Mouse back"),
        ("First image", "Home"),
        ("Last image", "End"),
    ]),
    ("Zoom & Rotate", &[
        ("Zoom in", "Ctrl++ / scroll up"),
        ("Zoom out", "Ctrl+− / scroll down"),
        ("Best fit", "Ctrl+0"),
        ("Original size (100%)", "Ctrl+1"),
        ("200%", "Ctrl+2"),
        ("Pan", "Drag / Ctrl+Arrows"),
        ("Rotate clockwise", "Ctrl+R"),
        ("Rotate counter-clockwise", "Ctrl+Shift+R"),
    ]),
    ("Image", &[
        ("Copy", "Ctrl+C"),
        ("Move to trash", "Delete"),
        ("Delete permanently", "Shift+Delete"),
        ("Open with…", "Ctrl+Shift+O"),
        ("Set as background", "Ctrl+F8"),
        ("Print", "Ctrl+P"),
        ("Reload", "F5"),
        ("Edit", "E"),
    ]),
    ("Editing", &[
        ("Rotate", "Ctrl+R / Ctrl+Shift+R"),
        ("Flip horizontally", "H"),
        ("Flip vertically", "V"),
        ("Save (replace)", "Ctrl+S / Ctrl+Enter"),
        ("Save as copy", "Ctrl+Shift+S"),
        ("Cancel", "Esc"),
    ]),
];

impl eframe::App for GlanceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        self.handle_drops(&ctx);
        self.background_work(&ctx);
        self.handle_keys(&ctx);

        // Keep in sync if the OS changed fullscreen (e.g. Esc in some WMs).
        let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        if fs != self.fullscreen && ctx.input(|i| i.viewport().fullscreen.is_some()) {
            self.fullscreen = fs;
        }

        let now = ctx.input(|i| i.time);
        let show_header = !self.fullscreen
            || self.edit.is_some()
            || ctx.input(|i| i.pointer.hover_pos().map(|p| p.y < 60.0).unwrap_or(false))
            || !matches!(self.dialog, Dialog::None);
        if show_header {
            self.header(ui);
        }
        if self.show_props && self.edit.is_none() && self.current.is_some() {
            self.properties(ui);
        }
        self.image_area(ui);
        self.dialogs(&ctx);
        self.toast_ui(&ctx);
        self.run_shots(&ctx);
        if std::env::var_os("GLANCE_DEBUG").is_some() {
            eprintln!("frame_ms={:.2}", frame_start.elapsed().as_secs_f64() * 1000.0);
        }
        if std::env::var_os("GLANCE_BENCH").is_some() {
            ctx.request_repaint(); // redraw nonstop to measure frame rate
        }
        let _ = now;
    }
}
