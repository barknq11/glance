//! Desktop integration that differs between Windows and Linux:
//! wallpaper, "Open With", printing and showing a file in its folder.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallpaperStyle {
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
    Span,
}

impl WallpaperStyle {
    pub const ALL: [WallpaperStyle; 6] = [
        WallpaperStyle::Fill,
        WallpaperStyle::Fit,
        WallpaperStyle::Stretch,
        WallpaperStyle::Center,
        WallpaperStyle::Tile,
        WallpaperStyle::Span,
    ];
    pub fn label(self) -> &'static str {
        match self {
            WallpaperStyle::Fill => "Fill",
            WallpaperStyle::Fit => "Fit",
            WallpaperStyle::Stretch => "Stretch",
            WallpaperStyle::Center => "Center",
            WallpaperStyle::Tile => "Tile",
            WallpaperStyle::Span => "Span (all screens)",
        }
    }
}

#[cfg_attr(windows, allow(dead_code))]
pub fn file_uri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut s = abs.to_string_lossy().replace('\\', "/");
    // Strip the Windows verbatim prefix that canonicalize adds.
    if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn app_data_dir(sub: &str) -> Result<PathBuf, String> {
    #[cfg(windows)]
    let base = dirs::data_local_dir().map(|d| d.join("Glance"));
    #[cfg(not(windows))]
    let base = dirs::data_dir().map(|d| d.join("glance"));
    let dir = base.ok_or("Could not find the user data folder")?.join(sub);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Copy (or convert) the picture into our own folder, so the wallpaper keeps
/// working if the original is moved or deleted.
fn prepare_wallpaper(path: &Path) -> Result<PathBuf, String> {
    let dir = app_data_dir("wallpaper")?;
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let _ = std::fs::remove_file(e.path());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let orientation = crate::meta::read_exif(path).orientation;
    if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "bmp") && orientation <= 1 {
        let dest = dir.join(format!("wallpaper-{stamp}.{ext}"));
        std::fs::copy(path, &dest).map_err(|e| e.to_string())?;
        return Ok(dest);
    }
    let img = crate::loader::decode_full(path)?;
    if img.color().has_alpha() {
        let dest = dir.join(format!("wallpaper-{stamp}.png"));
        img.save(&dest).map_err(|e| e.to_string())?;
        Ok(dest)
    } else {
        let dest = dir.join(format!("wallpaper-{stamp}.jpg"));
        crate::edit::save_jpeg(&img, &dest, 95)?;
        Ok(dest)
    }
}

/// Set the desktop background. Returns a short description of what was done.
pub fn set_wallpaper(path: &Path, style: WallpaperStyle) -> Result<String, String> {
    let file = prepare_wallpaper(path)?;
    imp::set_wallpaper(&file, style)
}

pub fn show_in_folder(path: &Path) -> Result<(), String> {
    imp::show_in_folder(path)
}

pub fn print(path: &Path) -> Result<(), String> {
    imp::print(path)
}

pub fn new_window() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).spawn();
    }
}

pub fn open_url(url: &str) {
    let _ = open::that_detached(url);
}

/// A program that can open images (used by the Linux "Open With" list).
#[derive(Clone, Debug)]
pub struct AppEntry {
    pub name: String,
    #[cfg_attr(windows, allow(dead_code))]
    pub exec: String,
}

pub use imp::{launch_app, list_image_apps, open_with_dialog, NATIVE_OPEN_WITH};

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER, SW_SHOWNORMAL,
    };

    pub const NATIVE_OPEN_WITH: bool = true;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn set_reg(name: &str, value: &str) -> bool {
        let key = wide("Control Panel\\Desktop".as_ref());
        let name = wide(name.as_ref());
        let data = wide(value.as_ref());
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr() as *const _,
                (data.len() * 2) as u32,
            ) == 0
        }
    }

    pub fn set_wallpaper(file: &Path, style: WallpaperStyle) -> Result<String, String> {
        // These are the values Windows Settings itself writes.
        let (wallpaper_style, tile) = match style {
            WallpaperStyle::Fill => ("10", "0"),
            WallpaperStyle::Fit => ("6", "0"),
            WallpaperStyle::Stretch => ("2", "0"),
            WallpaperStyle::Center => ("0", "0"),
            WallpaperStyle::Tile => ("0", "1"),
            WallpaperStyle::Span => ("22", "0"),
        };
        set_reg("WallpaperStyle", wallpaper_style);
        set_reg("TileWallpaper", tile);
        let path = wide(file.as_os_str());
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                path.as_ptr() as *mut _,
                SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
            )
        };
        if ok != 0 {
            Ok("Desktop background updated".into())
        } else {
            Err(format!("Windows refused the wallpaper: {}", std::io::Error::last_os_error()))
        }
    }

    pub fn show_in_folder(path: &Path) -> Result<(), String> {
        // explorer.exe wants the /select argument unquoted-joined.
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn open_with_dialog(path: &Path) -> Result<(), String> {
        Command::new("rundll32.exe")
            .arg("shell32.dll,OpenAs_RunDLL")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn list_image_apps() -> Vec<AppEntry> {
        Vec::new()
    }

    pub fn launch_app(_app: &AppEntry, _path: &Path) -> Result<(), String> {
        Err("Not used on Windows".into())
    }

    fn shell_print(file: &Path) -> bool {
        let verb = wide("print".as_ref());
        let f = wide(file.as_os_str());
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                f.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        r as isize > 32
    }

    pub fn print(path: &Path) -> Result<(), String> {
        // Opens the Windows "Print Pictures" dialog. Formats Windows can't
        // print directly are converted to PNG first.
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "bmp" | "gif" | "tif" | "tiff") && shell_print(path) {
            return Ok(());
        }
        let img = crate::loader::decode_full(path)?;
        let tmp = app_data_dir("print")?.join("print.png");
        img.save(&tmp).map_err(|e| e.to_string())?;
        if shell_print(&tmp) {
            Ok(())
        } else {
            Err("Windows has no printing program for this picture".into())
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub const NATIVE_OPEN_WITH: bool = false;

    fn run(cmd: &str, args: &[&str]) -> bool {
        Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn output(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    fn desktop() -> String {
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase()
    }

    pub fn set_wallpaper(file: &Path, style: WallpaperStyle) -> Result<String, String> {
        let uri = file_uri(file);
        let path = file.to_string_lossy().to_string();
        let de = desktop();
        let gnome_opt = match style {
            WallpaperStyle::Fill => "zoom",
            WallpaperStyle::Fit => "scaled",
            WallpaperStyle::Stretch => "stretched",
            WallpaperStyle::Center => "centered",
            WallpaperStyle::Tile => "wallpaper",
            WallpaperStyle::Span => "spanned",
        };

        if de.contains("kde") || de.contains("plasma") {
            let mode = match style {
                WallpaperStyle::Fill | WallpaperStyle::Span => "preserveAspectCrop",
                WallpaperStyle::Fit | WallpaperStyle::Center => "preserveAspectFit",
                WallpaperStyle::Stretch => "stretch",
                WallpaperStyle::Tile => "tile",
            };
            if run("plasma-apply-wallpaperimage", &["--fill-mode", mode, &path])
                || run("plasma-apply-wallpaperimage", &[&path])
            {
                return Ok("Plasma wallpaper updated".into());
            }
        }
        if de.contains("xfce") {
            if let Some(list) = output("xfconf-query", &["-c", "xfce4-desktop", "-l"]) {
                let style_n = match style {
                    WallpaperStyle::Center => "1",
                    WallpaperStyle::Tile => "2",
                    WallpaperStyle::Stretch => "3",
                    WallpaperStyle::Fit => "4",
                    WallpaperStyle::Fill => "5",
                    WallpaperStyle::Span => "6",
                };
                let mut any = false;
                for prop in list.lines() {
                    if prop.ends_with("/last-image") {
                        any |= run("xfconf-query", &["-c", "xfce4-desktop", "-p", prop, "-s", &path]);
                        let sp = prop.replace("/last-image", "/image-style");
                        run("xfconf-query", &["-c", "xfce4-desktop", "-p", &sp, "-s", style_n]);
                    }
                }
                if any {
                    return Ok("Xfce wallpaper updated".into());
                }
            }
        }
        if de.contains("cinnamon") {
            if run("gsettings", &["set", "org.cinnamon.desktop.background", "picture-uri", &uri]) {
                run("gsettings", &["set", "org.cinnamon.desktop.background", "picture-options", gnome_opt]);
                return Ok("Cinnamon wallpaper updated".into());
            }
        }
        if de.contains("mate") {
            if run("gsettings", &["set", "org.mate.background", "picture-filename", &path]) {
                run("gsettings", &["set", "org.mate.background", "picture-options", gnome_opt]);
                return Ok("MATE wallpaper updated".into());
            }
        }
        // GNOME, Budgie, Unity, Pantheon and anything else using GNOME settings.
        if run("gsettings", &["set", "org.gnome.desktop.background", "picture-uri", &uri]) {
            run("gsettings", &["set", "org.gnome.desktop.background", "picture-uri-dark", &uri]);
            run("gsettings", &["set", "org.gnome.desktop.background", "picture-options", gnome_opt]);
            return Ok("Desktop background updated".into());
        }
        // The desktop portal (works in sandboxes and many desktops).
        let opts = "{'show-preview': <false>, 'set-on': <'both'>}";
        if run(
            "gdbus",
            &[
                "call", "--session", "--dest", "org.freedesktop.portal.Desktop",
                "--object-path", "/org/freedesktop/portal/desktop",
                "--method", "org.freedesktop.portal.Wallpaper.SetWallpaperURI", "", &uri, opts,
            ],
        ) {
            return Ok("Wallpaper set via the desktop portal".into());
        }
        let feh_mode = match style {
            WallpaperStyle::Fill | WallpaperStyle::Span => "--bg-fill",
            WallpaperStyle::Fit => "--bg-max",
            WallpaperStyle::Stretch => "--bg-scale",
            WallpaperStyle::Center => "--bg-center",
            WallpaperStyle::Tile => "--bg-tile",
        };
        if run("feh", &[feh_mode, &path]) {
            return Ok("Wallpaper set with feh".into());
        }
        Err("Could not find a way to set the wallpaper on this desktop".into())
    }

    pub fn show_in_folder(path: &Path) -> Result<(), String> {
        let uri = file_uri(path);
        let arr = format!("['{}']", uri.replace('\'', "%27"));
        if run(
            "gdbus",
            &[
                "call", "--session", "--dest", "org.freedesktop.FileManager1",
                "--object-path", "/org/freedesktop/FileManager1",
                "--method", "org.freedesktop.FileManager1.ShowItems", &arr, "",
            ],
        ) {
            return Ok(());
        }
        let dir = path.parent().ok_or("No folder")?;
        open::that_detached(dir).map_err(|e| e.to_string())
    }

    pub fn open_with_dialog(path: &Path) -> Result<(), String> {
        open::that_detached(path).map_err(|e| e.to_string())
    }

    fn app_dirs() -> Vec<PathBuf> {
        let mut dirs_v = Vec::new();
        if let Some(d) = dirs::data_dir() {
            dirs_v.push(d.join("applications"));
            dirs_v.push(d.join("flatpak/exports/share/applications"));
        }
        let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        for d in sys.split(':').filter(|d| !d.is_empty()) {
            dirs_v.push(Path::new(d).join("applications"));
        }
        dirs_v.push("/var/lib/flatpak/exports/share/applications".into());
        dirs_v
    }

    /// Programs whose .desktop file says they open images.
    pub fn list_image_apps() -> Vec<AppEntry> {
        let mut seen = std::collections::HashSet::new();
        let mut apps = Vec::new();
        for dir in app_dirs() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                    continue;
                }
                let id = p.file_name().unwrap().to_string_lossy().to_string();
                if !seen.insert(id) {
                    continue; // earlier dirs take priority
                }
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                let (mut name, mut exec, mut mime, mut hidden, mut in_entry) =
                    (None, None, String::new(), false, false);
                for line in text.lines() {
                    let line = line.trim();
                    if line.starts_with('[') {
                        in_entry = line == "[Desktop Entry]";
                        continue;
                    }
                    if !in_entry {
                        continue;
                    }
                    if let Some(v) = line.strip_prefix("Name=") {
                        name.get_or_insert_with(|| v.to_string());
                    } else if let Some(v) = line.strip_prefix("Exec=") {
                        exec = Some(v.to_string());
                    } else if let Some(v) = line.strip_prefix("MimeType=") {
                        mime = v.to_string();
                    } else if line == "NoDisplay=true" || line == "Hidden=true" {
                        hidden = true;
                    }
                }
                if hidden || !mime.split(';').any(|m| m.starts_with("image/")) {
                    continue;
                }
                if let (Some(name), Some(exec)) = (name, exec) {
                    if !exec.contains("glance") {
                        apps.push(AppEntry { name, exec });
                    }
                }
            }
        }
        apps.sort_by_key(|a| a.name.to_lowercase());
        apps
    }

    pub fn launch_app(app: &AppEntry, path: &Path) -> Result<(), String> {
        let file = path.to_string_lossy().to_string();
        let uri = file_uri(path);
        let mut args: Vec<String> = Vec::new();
        let mut used = false;
        for tok in app.exec.split_whitespace() {
            match tok {
                "%f" | "%F" => {
                    args.push(file.clone());
                    used = true;
                }
                "%u" | "%U" => {
                    args.push(uri.clone());
                    used = true;
                }
                t if t.starts_with('%') => {}
                t => args.push(t.trim_matches('"').to_string()),
            }
        }
        if !used {
            args.push(file);
        }
        let (cmd, rest) = args.split_first().ok_or("Empty command")?;
        Command::new(cmd).args(rest).spawn().map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn print(path: &Path) -> Result<(), String> {
        // Send to the default CUPS printer, scaled to fit the page.
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let file = if matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
            path.to_path_buf()
        } else {
            let img = crate::loader::decode_full(path)?;
            let tmp = app_data_dir("print")?.join("print.png");
            img.save(&tmp).map_err(|e| e.to_string())?;
            tmp
        };
        let f = file.to_string_lossy().to_string();
        if run("lp", &["-o", "fit-to-page", &f]) || run("lpr", &[&f]) {
            Ok(())
        } else {
            Err("Printing needs CUPS (the 'lp' command) and a default printer".into())
        }
    }
}
