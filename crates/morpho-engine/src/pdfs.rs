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
            // page files always carry a numeric suffix (`stem-1.png`); an
            // unsuffixed file that happens to share the stem is NOT ours —
            // counting it as a page inflates the total and breaks the
            // single-page rename below
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with(&format!("{stem}-")))
                .unwrap_or(false)
                && p.extension().and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case(if target == Format::Jpg { "jpg" } else { "png" }))
                    .unwrap_or(false)
        })
        .collect();
    pages.sort_by_key(|p| natural_page_key(p.as_path()));
    if pages.is_empty() {
        return Err(Error::Other("poppler produced no pages".into()));
    }
    Ok(pages)
}

fn natural_page_key(p: &Path) -> u32 {
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
    pages.sort_by_key(|p| natural_page_key(p.as_path()));
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


/// Total pages via qpdf --show-npages.
pub async fn page_count(qpdf: &Path, src: &Path) -> Result<u32> {
    let outp = Command::new(qpdf)
        .arg("--show-npages")
        .arg(src)
        .output()
        .await?;
    if !outp.status.success() {
        return Err(Error::ProcessFailed {
            engine: "qpdf".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    let text = String::from_utf8_lossy(&outp.stdout).trim().to_string();
    text.parse::<u32>()
        .map_err(|_| Error::Other(format!("page count unreadable: {text}")))
}

/// Rotate pages. `degrees` must be 90/180/270; `pages` is a qpdf page spec
/// (e.g. "1-3", "1,4"), or None for all pages.
pub async fn rotate(
    qpdf: &Path,
    src: &Path,
    out: &Path,
    degrees: u32,
    pages: Option<&str>,
) -> Result<()> {
    if !matches!(degrees, 90 | 180 | 270) {
        return Err(Error::Other(format!("rotation must be 90, 180 or 270, got {degrees}")));
    }
    let range = pages.unwrap_or("1-z");
    let args = vec![
        src.display().to_string(),
        format!("--rotate={degrees}:{range}"),
        "--".into(),
        out.display().to_string(),
    ];
    qpdf_run(qpdf, &args).await
}

/// Delete the given pages (comma-separated, ranges with `-`), keeping the
/// rest in their original order.
pub async fn delete_pages(qpdf: &Path, src: &Path, out: &Path, delete_spec: &str) -> Result<()> {
    let total = page_count(qpdf, src).await?;
    let keep = invert_spec(delete_spec, total)?;
    let args = vec![
        src.display().to_string(),
        "--pages".into(),
        src.display().to_string(),
        keep,
        "--".into(),
        out.display().to_string(),
    ];
    qpdf_run(qpdf, &args).await
}

/// Reorder pages: `order` lists the desired sequence, e.g. "3,1,2".
pub async fn reorder(qpdf: &Path, src: &Path, out: &Path, order: &str) -> Result<()> {
    let mut args: Vec<String> = vec![src.display().to_string(), "--pages".into()];
    for tok in order.split(",").map(str::trim).filter(|t| !t.is_empty()) {
        let n = tok
            .parse::<u32>()
            .map_err(|_| Error::Other(format!("invalid page number: {tok}")))?;
        if n == 0 {
            return Err(Error::Other("page numbers start at 1".into()));
        }
        // qpdf requires the file name before every page range in the list
        args.push(src.display().to_string());
        args.push(tok.to_string());
    }
    if args.len() <= 2 {
        return Err(Error::Other("empty page order".into()));
    }
    args.push("--".into());
    args.push(out.display().to_string());
    qpdf_run(qpdf, &args).await
}

/// Shrink file size: recompress streams and downscale oversized images.
pub async fn compress(qpdf: &Path, src: &Path, out: &Path) -> Result<()> {
    let args = vec![
        src.display().to_string(),
        "--optimize-images".into(),
        "--stream-data=compress".into(),
        "--".into(),
        out.display().to_string(),
    ];
    qpdf_run(qpdf, &args).await
}

/// Turn a delete spec ("1,3-5") into a keep spec ("2,6-z") for --pages.
fn invert_spec(spec: &str, total: u32) -> Result<String> {
    let mut deleted = vec![false; (total + 1) as usize];
    for tok in spec.split(",").map(str::trim).filter(|t| !t.is_empty()) {
        let bad = || Error::Other(format!("invalid page number: {tok}"));
        let (a, b) = match tok.split_once("-") {
            Some((a, b)) => (
                a.trim().parse::<u32>().map_err(|_| bad())?,
                b.trim().parse::<u32>().map_err(|_| bad())?,
            ),
            None => {
                let n = tok.parse::<u32>().map_err(|_| bad())?;
                (n, n)
            }
        };
        if a == 0 || b < a || b > total {
            return Err(Error::Other(format!(
                "invalid page range: {tok} (document has {total} pages)"
            )));
        }
        for p in a..=b {
            deleted[p as usize] = true;
        }
    }
    let mut keep = String::new();
    let mut run: Option<(u32, u32)> = None;
    for p in 1..=total {
        if deleted[p as usize] {
            if let Some((a, b)) = run.take() {
                push_range(&mut keep, a, b);
            }
        } else {
            run = match run {
                Some((a, b)) => Some((a, b + 1)),
                None => Some((p, p)),
            };
        }
    }
    if let Some((a, b)) = run.take() {
        push_range(&mut keep, a, b);
    }
    if keep.is_empty() {
        return Err(Error::Other("cannot delete every page".into()));
    }
    keep.pop(); // trailing comma
    Ok(keep)
}

fn push_range(out: &mut String, a: u32, b: u32) {
    if a == b {
        out.push_str(&format!("{a},"));
    } else {
        out.push_str(&format!("{a}-{b},"));
    }
}
