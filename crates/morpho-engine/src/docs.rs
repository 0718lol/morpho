//! Pandoc bridge: markdown / html / txt / epub / docx / odt / rtf family.

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

use crate::error::{Error, Result};
use crate::format::Format;

fn pandoc_name(f: Format) -> Option<&'static str> {
    Some(match f {
        Format::Md => "markdown",
        Format::Html => "html",
        Format::Txt => "plain",
        Format::Rtf => "rtf",
        Format::Epub => "epub",
        Format::Docx => "docx",
        Format::Odt => "odt",
        _ => return None,
    })
}

/// Convert `src` to `target` (must be a pandoc-supported family) into `out_dir`.
pub fn convert_args(src: &Path, target: Format, out_file: &Path) -> Option<Vec<String>> {
    let to = pandoc_name(target)?;
    let mut args: Vec<String> = vec!["-t".into(), to.into()];
    // pandoc infers the reader from the source extension
    if matches!(target, Format::Html | Format::Epub) {
        args.push("-s".into()); // standalone document
    }
    if target == Format::Epub {
        let title = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Document");
        args.push("--metadata".into());
        args.push(format!("title={title}"));
    }
    args.push("-o".into());
    args.push(out_file.display().to_string());
    args.push(src.display().to_string());
    Some(args)
}

pub async fn run(pandoc: &Path, args: &[String]) -> Result<()> {
    let mut cmd = Command::new(pandoc);
    cmd.args(args)
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
            engine: "pandoc".into(),
            code: outp.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&outp.stderr).to_string(),
        });
    }
    Ok(())
}
