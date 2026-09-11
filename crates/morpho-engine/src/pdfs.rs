//! Poppler bridge: pdf -> text / images. Qpdf bridge: merge / split / encrypt.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::error::{Error, Result};
use crate::format::Format;

fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// pdf -> txt. `layout` keeps multi-column reading order heuristics.
pub async fn pdf_to_text(pdftotext: &Path, src: &Path, out: &Path, layout: bool) -> Result<()> {
    let mut cmd = Command::new(pdftotext);
    if layout {
        cmd.arg("-layout");
    } else {
        cmd.arg("-raw");
    }
    cmd.arg(src).arg(out);
    no_window(&mut cmd);
    let outp = cmd.output().await?;
    if !outp.status.success() || !out.exists() {
        return Err(Error::ProcessFailed {
            engine: "pdftotext".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    Ok(())
}

/// pdf -> one image per page, named `<stem>-<n>.png|jpg` in `out_dir`.
pub async fn pdf_to_images(
    pdftoppm: &Path,
    src: &Path,
    out_dir: &Path,
    stem: &str,
    target: Format,
    dpi: u32,
) -> Result<Vec<PathBuf>> {
    let ext = match target {
        Format::Jpg => "jpeg",
        _ => "png",
    };
    let prefix = out_dir.join(stem);
    let mut cmd = Command::new(pdftoppm);
    cmd.arg("-r").arg(dpi.to_string())
        .arg(format!("-{ext}"))
        .arg(src)
        .arg(&prefix);
    no_window(&mut cmd);
    let outp = cmd.output().await?;
    if !outp.status.success() {
        return Err(Error::ProcessFailed {
            engine: "pdftoppm".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    let mut pages: Vec<PathBuf> = std::fs::read_dir(out_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s == stem || s.starts_with(&format!("{stem}-")))
                .unwrap_or(false)
                && p.extension().and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case(if target == Format::Jpg { "jpg" } else { "png" }))
                    .unwrap_or(false)
        })
        .collect();
    pages.sort_by_key(natural_page_key);
    if pages.is_empty() {
        return Err(Error::Other("poppler produced no pages".into()));
    }
    Ok(pages)
}

fn natural_page_key(p: &PathBuf) -> u32 {
    p.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit('-').next().and_then(|n| n.parse::<u32>().ok()))
        .unwrap_or(0)
}

// ---------------- qpdf ----------------

pub async fn qpdf_run(qpdf: &Path, args: &[String]) -> Result<()> {
    let mut cmd = Command::new(qpdf);
    cmd.args(args);
    no_window(&mut cmd);
    let outp = cmd.output().await?;
    if !outp.status.success() {
        return Err(Error::ProcessFailed {
            engine: "qpdf".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    Ok(())
}

/// Merge many pdfs into one.
pub async fn merge(qpdf: &Path, inputs: &[PathBuf], out: &Path) -> Result<()> {
    let mut args: Vec<String> = vec!["--empty".into(), "--pages".into()];
    for i in inputs {
        args.push(i.display().to_string());
        args.push("1-z".into());
    }
    args.push("--".into());
    args.push(out.display().to_string());
    qpdf_run(qpdf, &args).await
}

/// Split into one file per page: `<stem>-<n>.pdf` in `out_dir`.
pub async fn split(qpdf: &Path, src: &Path, out_dir: &Path, stem: &str) -> Result<Vec<PathBuf>> {
    let pattern = out_dir.join(format!("{stem}-%d.pdf"));
    qpdf_run(qpdf, &["--split-pages".into(), src.display().to_string(), pattern.display().to_string()]).await?;
    let mut pages: Vec<PathBuf> = std::fs::read_dir(out_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_stem().and_then(|s| s.to_str())
                .map(|s| s.starts_with(&format!("{stem}-")) && s != stem)
                .unwrap_or(false)
        })
        .collect();
    pages.sort_by_key(natural_page_key);
    Ok(pages)
}

/// AES-256 encryption with a user password (owner password defaults to it).
pub async fn encrypt(
    qpdf: &Path,
    src: &Path,
    out: &Path,
    user_pw: &str,
    owner_pw: Option<&str>,
) -> Result<()> {
    let owner = owner_pw.unwrap_or(user_pw);
    let args = vec![
        "--encrypt".into(), user_pw.into(), owner.into(), "256".into(), "--".into(),
        src.display().to_string(), out.display().to_string(),
    ];
    qpdf_run(qpdf, &args).await
}

/// Remove encryption; supply the password if the file is protected.
pub async fn decrypt(qpdf: &Path, src: &Path, out: &Path, password: Option<&str>) -> Result<()> {
    let mut args: Vec<String> = Vec::new();
    if let Some(p) = password {
        args.push(format!("--password={p}"));
    }
    args.push("--decrypt".into());
    args.push(src.display().to_string());
    args.push(out.display().to_string());
    qpdf_run(qpdf, &args).await
}
