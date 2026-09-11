//! LibreOffice headless bridge: office docs, spreadsheets, slides, images -> pdf.
//!
//! One fresh user profile per invocation keeps runs isolated; a global async
//! gate serializes instances so soffice never fights itself.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use tokio::process::Command;
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
        let _gate = self.gate.lock().await;
        let profile = tempfile::tempdir()?;
        let profile_url = path_to_file_url(&profile.path().join("profile"));

        let mut cmd = Command::new(&self.soffice);
        cmd.args([
            "--headless",
            "--norestore",
            "--nolockcheck",
            "--nodefault",
            "--nologo",
            "--nofirststartwizard",
        ])
        .arg(format!("-env:UserInstallation={profile_url}"))
        .arg(format!("--convert-to={}", target.extension()))
        .arg(format!("--outdir={}", out_dir.display()))
        .args(inputs)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let output = cmd.output().await?;
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
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            return Err(Error::ProcessFailed {
                engine: "libreoffice".into(),
                code: output.status.code().unwrap_or(-1),
                stderr: if stderr.is_empty() { stdout } else { stderr },
            });
        }
        Ok(results)
    }
}

fn path_to_file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    format!("file:///{s}")
}
