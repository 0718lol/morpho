//! Locate bundled conversion engines (ffmpeg, libreoffice, poppler, ...).
//!
//! Search order:
//! 1. `MORPHO_ENGINES_DIR` env override
//! 2. `<exe dir>/engines`   (Tauri resource layout on Windows)
//! 3. `<repo>/engines`      (development checkout / CLI from source)

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Engines {
    pub dir: PathBuf,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

impl Engines {
    pub fn discover() -> Option<Engines> {
        if let Some(dir) = std::env::var_os("MORPHO_ENGINES_DIR") {
            let dir = PathBuf::from(dir);
            if dir.is_dir() {
                return Some(Engines { dir });
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let dir = parent.join("engines");
                if dir.is_dir() {
                    return Some(Engines { dir });
                }
            }
        }
        // dev fallback: <repo>/engines two levels above this crate
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = manifest
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("engines"));
        if let Some(dir) = dir {
            if dir.is_dir() {
                return Some(Engines { dir });
            }
        }
        None
    }

    /// Like [`discover`] but fails with a precise error listing the searched dirs.
    pub fn discover_or_err() -> Result<Engines> {
        Self::discover().ok_or_else(|| {
            Error::EngineMissing(
                "any".into(),
                std::env::var("MORPHO_ENGINES_DIR").unwrap_or_else(|_| "<exe>/engines".into()),
            )
        })
    }

    pub fn optional(&self, rel: &str) -> Option<PathBuf> {
        let p = self.dir.join(rel);
        p.is_file().then_some(p)
    }

    pub fn require(&self, rel: &str) -> Result<PathBuf> {
        self.optional(rel).ok_or_else(|| {
            Error::EngineMissing(
                rel.to_string(),
                self.dir.display().to_string(),
            )
        })
    }

    pub fn ffmpeg(&self) -> Result<PathBuf> {
        self.require(&format!("ffmpeg/{}", exe("ffmpeg")))
    }

    pub fn ffprobe(&self) -> Result<PathBuf> {
        self.require(&format!("ffmpeg/{}", exe("ffprobe")))
    }

    pub fn soffice(&self) -> Option<PathBuf> {
        self.optional(&format!("libreoffice/program/{}", exe("soffice")))
            .or_else(|| self.optional(&format!("libreoffice/program/{}", exe("soffice.bin"))))
    }

    pub fn pdftoppm(&self) -> Option<PathBuf> {
        self.optional(&format!("poppler/{}", exe("pdftoppm")))
    }

    pub fn pdftotext(&self) -> Option<PathBuf> {
        self.optional(&format!("poppler/{}", exe("pdftotext")))
    }

    pub fn tesseract(&self) -> Option<PathBuf> {
        self.optional(&format!("tesseract/{}", exe("tesseract")))
    }

    pub fn tessdata(&self) -> Option<PathBuf> {
        let dir = self.dir.join("tesseract/tessdata");
        dir.is_dir().then_some(dir)
    }

    pub fn pandoc(&self) -> Option<PathBuf> {
        self.optional(&format!("pandoc/{}", exe("pandoc")))
    }

    pub fn qpdf(&self) -> Option<PathBuf> {
        self.optional(&format!("qpdf/{}", exe("qpdf")))
    }

    /// Per-engine availability report for the UI.
    pub fn status(&self) -> Vec<(String, bool)> {
        vec![
            ("ffmpeg".into(), self.ffmpeg().is_ok()),
            ("libreoffice".into(), self.soffice().is_some()),
            ("poppler".into(), self.pdftoppm().is_some() && self.pdftotext().is_some()),
            ("tesseract".into(), self.tesseract().is_some()),
            ("pandoc".into(), self.pandoc().is_some()),
            ("qpdf".into(), self.qpdf().is_some()),
        ]
    }
}

/// Env-file helper: engines like tesseract need their data dir on PATH env.
pub fn with_env(cmd: &mut tokio::process::Command, key: &str, val: &Path) {
    cmd.env(key, val);
}
