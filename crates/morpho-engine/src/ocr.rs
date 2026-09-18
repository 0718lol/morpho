//! Tesseract OCR bridge: image/pdf-adjacent raster -> txt.

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

use crate::error::{Error, Result};

/// OCR an image to `out.txt`. Languages like "eng" or "eng+chi_sim".
pub async fn ocr(
    tesseract: &Path,
    tessdata: &Path,
    src: &Path,
    out_txt: &Path,
    langs: &str,
) -> Result<()> {
    let mut cmd = Command::new(tesseract);
    cmd.arg(src)
        .arg("stdout")
        .arg("-l").arg(langs)
        .arg("--tessdata-dir").arg(tessdata)
        .arg("--psm").arg("3")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let outp = cmd.output().await?;
    if !outp.status.success() {
        return Err(Error::ProcessFailed {
            engine: "tesseract".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    std::fs::write(out_txt, &outp.stdout)?;
    Ok(())
}

/// OCR an image, returning the recognized text (used for per-page OCR
/// when aggregating multi-page scans).
pub async fn ocr_to_string(
    tesseract: &Path,
    tessdata: &Path,
    src: &Path,
    langs: &str,
) -> Result<String> {
    let mut cmd = Command::new(tesseract);
    cmd.arg(src)
        .arg("stdout")
        .arg("-l").arg(langs)
        .arg("--tessdata-dir").arg(tessdata)
        .arg("--psm").arg("3")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let outp = cmd.output().await?;
    if !outp.status.success() {
        return Err(Error::ProcessFailed {
            engine: "tesseract".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&outp.stdout).into_owned())
}

/// OCR an image into a searchable PDF: the original page image plus an
/// invisible text layer. Tesseract derives the output filename from the
/// base path (`<base>.pdf`), so `out_pdf` must end in `.pdf`.
pub async fn ocr_pdf(
    tesseract: &Path,
    tessdata: &Path,
    src: &Path,
    out_pdf: &Path,
    langs: &str,
) -> Result<()> {
    let base = out_pdf.with_extension("");
    let mut cmd = Command::new(tesseract);
    cmd.arg(src)
        .arg(&base)
        .arg("-l").arg(langs)
        .arg("--tessdata-dir").arg(tessdata)
        .arg("--psm").arg("3")
        .arg("pdf")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let outp = cmd.output().await?;
    if !outp.status.success() || !out_pdf.exists() {
        return Err(Error::ProcessFailed {
            engine: "tesseract".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    Ok(())
}

/// Pick available languages, preferring "eng+chi_sim" when the data is there.
pub fn detect_langs(tessdata: &Path) -> String {
    let mut langs = vec!["eng"];
    if tessdata.join("chi_sim.traineddata").exists() {
        langs.push("chi_sim");
    }
    langs.join("+")
}
