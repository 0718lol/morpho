//! PDF -> Word (docx) with layout preservation.
//!
//! Pipeline: `pdftohtml -xml -i` (bundled poppler) extracts every text chunk
//! with position/font/size/color plus raster images; this module re-flows the
//! positioned chunks into lines, paragraphs, lists and simple tables, then
//! emits real OOXML via docx-rs. Pure Rust — no office suite in the loop.

pub mod emit;
pub mod layout;
pub mod parser;

use std::path::Path;

use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};

/// Convert `src.pdf` to `dst.docx`.
pub async fn convert(
    pdftohtml: &Path,
    src: &Path,
    dst: &Path,
    token: &CancellationToken,
    report: &mut (dyn FnMut(f32, &str) + Send),
) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().join("doc");
    // pdftohtml writes `<base>.xml` plus, with -i, the extracted images it
    // references from the xml `src=` attributes — all inside our tempdir.
    run_pdftohtml(pdftohtml, src, &base).await?;
    report(0.25, "layout");

    let doc = parser::parse(&base.with_extension("xml"), tmp.path())?;
    let flow = layout::reflow(doc, token, report)?;
    if token.is_cancelled() {
        return Err(Error::Cancelled);
    }

    report(0.85, "docx");
    emit::emit_docx(&flow, dst)?;
    report(1.0, "");
    Ok(())
}

async fn run_pdftohtml(pdftohtml: &Path, src: &Path, base: &Path) -> Result<()> {
    let mut cmd = tokio::process::Command::new(pdftohtml);
    // -xml: positioned chunk XML; images are extracted and referenced too
    // (NOT with -i — that flag means "ignore images")
    cmd.arg("-xml")
        .arg(src)
        .arg(base)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let outp = cmd.output().await?;
    let xml = base.with_extension("xml");
    if !outp.status.success() || !xml.exists() {
        return Err(Error::ProcessFailed {
            engine: "pdftohtml".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    Ok(())
}
