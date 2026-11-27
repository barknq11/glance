//! The list of images being browsed: every image in the opened file's folder,
//! sorted in natural filename order (so "img2" comes before "img10").

use std::path::{Path, PathBuf};

pub const EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "dib", "ico", "cur", "tif",
    "tiff", "tga", "pnm", "pbm", "pgm", "ppm", "pam", "qoi", "hdr", "exr", "dds", "ff", "svg", "svgz",
];

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn is_hidden(path: &Path, md: Option<&std::fs::Metadata>) -> bool {
    let dot = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Some(m) = md {
            if m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0 {
                return true;
            }
        }
    }
    #[cfg(not(windows))]
    let _ = md;
    dot
}

fn sort_key(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn sort(files: &mut [PathBuf]) {
    files.sort_by(|a, b| natord::compare(&sort_key(a), &sort_key(b)));
}

#[derive(Default)]
pub struct Folder {
    pub dir: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    /// When the user opened several explicit files we browse just those.
    explicit: bool,
}

impl Folder {
    /// Browse the folder that contains `file`.
    pub fn for_file(file: &Path) -> Self {
        let dir = file.parent().map(Path::to_path_buf);
        let mut f = Folder { dir, files: vec![], explicit: false };
        f.rescan();
        if f.index_of(file).is_none() {
            f.files.push(file.to_path_buf());
            sort(&mut f.files);
        }
        f
    }

    /// Browse a folder directly; returns the first image, if any.
    pub fn for_dir(dir: &Path) -> Self {
        let mut f = Folder { dir: Some(dir.to_path_buf()), files: vec![], explicit: false };
        f.rescan();
        f
    }

    /// Browse exactly the given files.
    pub fn for_files(mut files: Vec<PathBuf>) -> Self {
        sort(&mut files);
        files.dedup();
        Folder { dir: None, files, explicit: true }
    }

    /// Re-read the folder from disk (blocking). Returns true if the list changed.
    pub fn rescan(&mut self) -> bool {
        let files = list(self.dir.as_deref(), self.explicit.then(|| self.files.clone()));
        self.apply(files)
    }

    /// What a background scan needs: (folder, explicit file list).
    pub fn scan_job(&self) -> (Option<PathBuf>, Option<Vec<PathBuf>>) {
        (self.dir.clone(), self.explicit.then(|| self.files.clone()))
    }

    /// Replace the list with a scan result. Returns true if it changed.
    pub fn apply(&mut self, files: Option<Vec<PathBuf>>) -> bool {
        match files {
            Some(f) if f != self.files => {
                self.files = f;
                true
            }
            _ => false,
        }
    }

    /// Position of `path` in the list. Plain comparisons only: this runs
    /// several times per frame, so it must never touch the disk.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        let want_raw = path.as_os_str();
        if let Some(i) = self.files.iter().position(|p| p.as_os_str() == want_raw) {
            return Some(i);
        }
        // Windows paths are case-insensitive.
        #[cfg(windows)]
        {
            let want = path.as_os_str().to_string_lossy().to_lowercase();
            return self.files.iter().position(|p| p.as_os_str().to_string_lossy().to_lowercase() == want);
        }
        #[cfg(not(windows))]
        None
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn get(&self, i: usize) -> Option<&PathBuf> {
        self.files.get(i)
    }
}


/// List a folder's images (or re-check an explicit list). Returns None if
/// the folder can't be read. Safe to call from a background thread.
pub fn list(dir: Option<&Path>, explicit: Option<Vec<PathBuf>>) -> Option<Vec<PathBuf>> {
    if let Some(mut files) = explicit {
        files.retain(|p| p.exists());
        return Some(files);
    }
    let entries = std::fs::read_dir(dir?).ok()?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let md = e.metadata().ok();
            let is_file = md.as_ref().map(|m| m.is_file()).unwrap_or(false)
                || e.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            let p = e.path();
            (is_file && is_image(&p) && !is_hidden(&p, md.as_ref())).then_some(p)
        })
        .collect();
    sort(&mut files);
    Some(files)
}
