//! LibreOffice headless bridge: office docs, spreadsheets, slides, images -> pdf.
//!
//! One fresh user profile per invocation keeps runs isolated; a global async
//! gate serializes instances so soffice never fights itself.
//!
//! NOTE: this bridge deliberately uses std::process inside spawn_blocking.
//! tokio::process spawns hang LibreOffice's launcher restart chain on Windows,
//! while plain std::process works reliably.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::format::Format;

#[derive(Clone)]
pub struct LibreOffice {
    soffice: PathBuf,
    gate: Arc<Mutex<()>>,
}

impl LibreOffice {
    pub fn new(soffice: PathBuf) -> Self {
        Self { soffice, gate: Arc::new(Mutex::new(())) }
    }

    /// Convert each input to `target` into `out_dir`, returning produced paths.
    pub async fn convert(
        &self,
        inputs: &[PathBuf],
        target: Format,
        out_dir: &Path,
    ) -> Result<Vec<PathBuf>> {
        let soffice = self.soffice.clone();
        let inputs: Vec<PathBuf> = inputs.iter().map(|p| absolutize(p)).collect();
        let out_dir = absolutize(out_dir);
        let _gate = self.gate.lock().await;
        let res = tokio::task::spawn_blocking(move || run_soffice(&soffice, &inputs, target, &out_dir))
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        res
    }
}

fn run_soffice(
    soffice: &Path,
    inputs: &[PathBuf],
    target: Format,
    out_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let profile = tempfile::tempdir()?;
    let profile_url = path_to_file_url(&profile.path().join("profile"));

    // stdout+stderr go to temp files (read on failure)
    let err_path = profile.path().join("lo-output.log");
    let err_file = std::fs::File::create(&err_path)?;
    let out_file = std::fs::File::create(profile.path().join("lo-stdout.log"))?;

    // CRITICAL: soffice runs with cwd = program dir, so relative paths would
    // resolve against the wrong directory and LO hangs waiting on a file it
    // cannot see. Callers absolutize inputs/out_dir (absolutize above).
    let output = Command::new(soffice)
        .args([
            "--headless",
            "--norestore",
            "--nolockcheck",
            "--nodefault",
            "--nologo",
        ])
        .arg(format!("-env:UserInstallation={profile_url}"))
        .arg("--convert-to")
        .arg(target.extension())
        // NOTE: space form — the console launcher rejects --outdir=<path>
        .arg("--outdir")
        .arg(out_dir)
        .args(inputs)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .current_dir(soffice.parent().unwrap_or(Path::new(".")))
        .output()?;

    let mut results = Vec::new();
    for input in inputs {
        let produced = out_dir
            .join(input.file_stem().unwrap_or_default())
            .with_extension(target.extension());
        if produced.exists() {
            results.push(produced);
        }
    }
    if results.is_empty() {
        let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
        let stdout = std::fs::read_to_string(profile.path().join("lo-stdout.log"))
            .unwrap_or_default();
        let log = if stderr.is_empty() { stdout } else { stderr };
        return Err(Error::ProcessFailed {
            engine: "libreoffice".into(),
            code: output.status.code().unwrap_or(-1),
            stderr: log,
        });
    }
    Ok(results)
}

fn path_to_file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    format!("file:///{s}")
}

fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}
