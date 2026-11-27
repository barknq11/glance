# Glance

A fast, simple image viewer for **Windows and Linux**, modelled on the
behaviour of GNOME's [Loupe](https://gitlab.gnome.org/GNOME/loupe) image
viewer and written from scratch in Rust ([egui](https://github.com/emilk/egui)).
It is a single self-contained program: no GTK, no runtime to install.

Glance is an independent project and is not affiliated with GNOME.

## Features

| | |
|---|---|
| **Browsing** | Opens a file and lets you flip through every image in its folder, in natural filename order (`img2` before `img10`). New, renamed and deleted files are picked up automatically; the open image reloads when it changes on disk. Neighbouring images are decoded in the background so flipping is instant. |
| **Zoom & pan** | Best fit (never enlarges past 100%), 100%, 200%, up to 2000%. Smooth animated zoom towards the cursor with the mouse wheel, touchpad pinch or keyboard. Drag or Ctrl+arrows to pan. Crisp pixels at high zoom. |
| **Rotate** | Animated 90° view rotation. EXIF orientation is applied automatically. |
| **Formats** | PNG, APNG, JPEG, GIF, WebP, BMP, ICO, TIFF, TGA, PNM, QOI, HDR, OpenEXR, DDS, Farbfeld and SVG. Animated GIF / WebP / APNG play. Transparent images show a checkerboard. Pictures larger than the GPU limit are shown reduced. |
| **Properties** (F9) | Dimensions, format, colour depth, transparency; file name, folder, size, dates; camera, lens, exposure, ISO, focal length, date taken, creator, copyright; GPS coordinates with a link to OpenStreetMap. |
| **Editing** (E) | Crop (free or Original, Square, 16:9, 4:3, 3:2 and 5:4, landscape or portrait), rotate, flip. Save over the original or as “name (edited).ext”. Saves go through a temporary file, so a failed save never damages the original. |
| **Desktop integration** | Set as desktop background, Open With…, Show in folder, Print, Copy to clipboard, Move to Recycle Bin/Trash, Delete permanently, drag-and-drop, fullscreen, new window, light/dark theme following the system. |

### Platform-specific behaviour

| Feature | Windows | Linux |
|---|---|---|
| Set as background | Native Windows wallpaper API, with Fill / Fit / Stretch / Center / Tile / Span | GNOME, Budgie, Cinnamon, MATE, KDE Plasma and Xfce settings, then the desktop portal, then `feh` |
| Open With… | Windows' own “Open with” chooser | List of installed apps that open images (from `.desktop` files) |
| Print | Windows “Print Pictures” dialog | CUPS (`lp`), scaled to fit the page |
| Show in folder | File Explorer with the file selected | File manager via D-Bus, falling back to opening the folder |
| Trash | Recycle Bin | Freedesktop Trash |

The wallpaper image is copied into Glance's own data folder first
(`%LOCALAPPDATA%\Glance` or `~/.local/share/glance`), so the background keeps
working if the original is moved or deleted.

## Keyboard shortcuts

| Action | Keys |
|---|---|
| Next / previous image | Right / Left, Page Down / Page Up, mouse forward / back |
| First / last image | Home / End |
| Zoom in / out | Ctrl++ / Ctrl+−, mouse wheel |
| Best fit / 100% / 200% | Ctrl+0 / Ctrl+1 / Ctrl+2 |
| Pan | Drag, Ctrl+arrows |
| Rotate | Ctrl+R / Ctrl+Shift+R |
| Fullscreen | F11 or double-click; Esc leaves |
| Properties | F9 or Alt+Enter |
| Copy | Ctrl+C |
| Move to trash / delete permanently | Delete / Shift+Delete |
| Open with… | Ctrl+Shift+O |
| Set as background | Ctrl+F8 |
| Print | Ctrl+P |
| Reload | F5 |
| Edit | E, then H / V to flip, Ctrl+S to save, Ctrl+Shift+S to save a copy, Esc to cancel |
| Open, new window, close, quit | Ctrl+O, Ctrl+N, Ctrl+W, Ctrl+Q |
| All shortcuts | F1 |

## Install

### Windows

Unzip the release, then run:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

This installs for your user only (no admin): it copies `glance.exe` to
`%LOCALAPPDATA%\Programs\Glance`, adds a Start menu entry, and registers
Glance in “Open with” and in **Settings → Apps → Default apps**.
`glance.exe` also runs on its own without installing.
`uninstall.ps1` removes everything again.

### Linux

From an extracted release folder, or from the source folder (builds first):

```sh
./install.sh             # or: ./packaging/linux/install.sh
./install.sh --uninstall
```

This installs to `~/.local/bin`, adds a desktop entry and icons, and prints the
`xdg-mime` command to make Glance your default viewer.

If you just want GNOME's original app on Linux, it is available as a Flatpak:
`flatpak install flathub org.gnome.Loupe`.

## Build from source

You need [Rust](https://rustup.rs) (stable).

```sh
cargo build --release      # binary: target/release/glance(.exe)
cargo test
```

- **Windows**: works with either the MSVC or GNU toolchain. With GNU, the app
  icon is embedded into the .exe when `windres` is available (MSYS2's is
  picked up automatically from `C:\msys64`).
  If the project folder's path is very long, set a short output folder,
  e.g. `$env:CARGO_TARGET_DIR = "$env:TEMP\glance-target"`.
- **Linux**: needs a C linker (`build-essential` or similar). The usual
  desktop libraries (OpenGL/EGL, X11 or Wayland, xkbcommon) are loaded at
  run time.

`.github/workflows/build.yml` builds and tests both platforms on GitHub Actions
and uploads ready-to-use Windows `.zip` and Linux `.tar.gz` packages.

## Command line

```
glance [FILE|FOLDER]...      open files, or every image in a folder
glance --export-icon PATH [SIZE]
glance --version
```

## Project layout

| File | What it does |
|---|---|
| `src/app.rs` | Window, header bar, overlays, properties panel, editor UI, dialogs, shortcuts |
| `src/view.rs` | Zoom / pan / rotation maths and animation |
| `src/loader.rs` | Background decoding, animation frames, SVG rendering, cache |
| `src/folder.rs` | Folder listing and natural sort |
| `src/meta.rs` | File facts and EXIF details |
| `src/edit.rs` | Crop / rotate / flip and safe saving |
| `src/platform.rs` | Windows- and Linux-specific integration |
| `src/icon.rs`, `src/icons.rs` | App icon and UI icons, drawn in code |
| `build.rs` | Embeds the icon and version info into the Windows .exe |

## Differences from Loupe

Glance reproduces Loupe's behaviour and shortcuts, but it is not a port:

- Image decoding is in-process (Loupe sandboxes decoders on Linux).
- No HEIC/AVIF/JPEG XL or camera RAW support yet.
- Saving an edit does not keep EXIF metadata.
- GPS locations show coordinates and an OpenStreetMap link instead of an
  offline place name.
- Interface text is English only.
