// On Windows, embed the app icon and version info into glance.exe so
// File Explorer and the taskbar show it. Needs `windres` (MinGW/MSYS2) for
// the GNU toolchain or `rc.exe` for MSVC; if neither is found the build
// still succeeds, just without an embedded icon.

#[path = "src/icon.rs"]
mod icon;

use std::path::{Path, PathBuf};
use std::process::Command;

fn find_windres() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("windres"),
        PathBuf::from(r"C:\msys64\ucrt64\bin\windres.exe"),
        PathBuf::from(r"C:\msys64\mingw64\bin\windres.exe"),
    ];
    candidates
        .into_iter()
        .find(|c| Command::new(c).arg("--version").output().map(|o| o.status.success()).unwrap_or(false))
}

fn write_ico(path: &Path) {
    let frames: Vec<image::codecs::ico::IcoFrame> = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&s| {
            let img = icon::render(s);
            image::codecs::ico::IcoFrame::as_png(img.as_raw(), s, s, image::ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    let f = std::fs::File::create(path).unwrap();
    image::codecs::ico::IcoEncoder::new(f).encode_images(&frames).unwrap();
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("glance.ico");
    write_ico(&ico);
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let nums: Vec<&str> = version.split(['.', '-']).take(3).collect();
    let v = format!("{},{},{},0", nums[0], nums.get(1).unwrap_or(&"0"), nums.get(2).unwrap_or(&"0"));
    let rc = out.join("glance.rc");
    std::fs::write(
        &rc,
        format!(
            r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {v}
PRODUCTVERSION {v}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Glance Image Viewer"
      VALUE "ProductName", "Glance"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "glance.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
            ico = ico.display().to_string().replace('\\', "/"),
        ),
    )
    .unwrap();

    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if env == "gnu" {
        let Some(windres) = find_windres() else {
            println!("cargo:warning=windres not found; glance.exe will have no embedded icon");
            return;
        };
        let obj = out.join("glance-res.o");
        let ok = Command::new(windres)
            .args(["-O", "coff", "-i"])
            .arg(&rc)
            .arg("-o")
            .arg(&obj)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bins={}", obj.display());
        } else {
            println!("cargo:warning=windres failed; glance.exe will have no embedded icon");
        }
    } else {
        let res = out.join("glance.res");
        let ok = Command::new("rc.exe")
            .arg("/nologo")
            .arg("/fo")
            .arg(&res)
            .arg(&rc)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        } else {
            println!("cargo:warning=rc.exe not found; glance.exe will have no embedded icon");
        }
    }
}
