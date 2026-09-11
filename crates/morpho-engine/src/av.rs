//! Audio/video pipeline through the bundled FFmpeg sidecar.
//!
//! Features: per-target codec maps, quality/preset knobs, real-time progress
//! via `-progress pipe:1`, two-pass palette GIF for high quality, cooperative
//! cancellation.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::format::Format;

#[derive(Debug, Clone, Default)]
pub struct AvOptions {
    /// 1..=100, None = engine default
    pub quality: Option<u8>,
    /// "wechat" | "web" | "archive"
    pub preset: Option<String>,
}

fn video_args(target: Format, opts: &AvOptions) -> Vec<String> {
    let crf = match opts.quality {
        Some(q) => (51 - (q as f32 * 34.0 / 100.0).round() as i32).clamp(14, 51).to_string(),
        None => "23".to_string(),
    };
    let mut scale = String::new();
    let mut extra: Vec<String> = vec![];
    match opts.preset.as_deref() {
        Some("wechat") => {
            scale = "-vf".into();
            extra = vec!["scale='min(1280,iw)':-2".into()];
        }
        Some("archive") => {
            scale = "-vf".into();
            extra = vec!["scale='min(1920,iw)':-2".into()];
        }
        _ => {}
    }
    match target {
        Format::Mp4 | Format::Mov | Format::M4v => {
            let mut v = vec![
                "-c:v".into(), "libx264".into(),
                "-preset".into(), "medium".into(),
                "-crf".into(), crf,
                "-pix_fmt".into(), "yuv420p".into(),
                "-c:a".into(), "aac".into(), "-b:a".into(), "192k".into(),
                "-movflags".into(), "+faststart".into(),
            ];
            if !scale.is_empty() {
                v.push(scale);
                v.extend(extra);
            }
            v
        }
        Format::Mkv => vec![
            "-c:v".into(), "libx264".into(),
            "-preset".into(), "medium".into(),
            "-crf".into(), crf,
            "-pix_fmt".into(), "yuv420p".into(),
            "-c:a".into(), "aac".into(), "-b:a".into(), "192k".into(),
        ],
        Format::Webm => vec![
            "-c:v".into(), "libvpx-vp9".into(),
            "-crf".into(), "34".into(), "-b:v".into(), "0".into(), "-row-mt".into(), "1".into(),
            "-c:a".into(), "libopus".into(), "-b:a".into(), "128k".into(),
        ],
        Format::Avi => vec![
            "-c:v".into(), "mpeg4".into(), "-q:v".into(), "5".into(),
            "-c:a".into(), "libmp3lame".into(), "-b:a".into(), "192k".into(),
        ],
        Format::Wmv => vec![
            "-c:v".into(), "wmv2".into(), "-b:v".into(), "2500k".into(),
            "-c:a".into(), "wmav2".into(), "-b:a".into(), "192k".into(),
        ],
        Format::Flv => vec![
            "-c:v".into(), "libx264".into(), "-preset".into(), "veryfast".into(),
            "-pix_fmt".into(), "yuv420p".into(),
            "-c:a".into(), "aac".into(), "-b:a".into(), "128k".into(),
        ],
        _ => vec![],
    }
}

fn audio_args(target: Format, opts: &AvOptions) -> Vec<String> {
    let br = match opts.quality {
        Some(q) => format!("{}k", (96u32 + q as u32).clamp(96, 320)),
        None => "192k".to_string(),
    };
    match target {
        Format::Mp3 => vec!["-c:a".into(), "libmp3lame".into(), "-b:a".into(), br],
        Format::Wav => vec!["-c:a".into(), "pcm_s16le".into()],
        Format::Flac => vec!["-c:a".into(), "flac".into()],
        Format::Ogg => vec!["-c:a".into(), "libvorbis".into(), "-b:a".into(), br],
        Format::Opus => vec!["-c:a".into(), "libopus".into(), "-b:a".into(), "128k".into()],
        Format::M4a | Format::Aac => vec!["-c:a".into(), "aac".into(), "-b:a".into(), br],
        _ => vec![],
    }
}

pub async fn probe_duration(ffprobe: &Path, src: &Path) -> Result<Option<f64>> {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(src)
        .output()
        .await?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(text.parse::<f64>().ok())
}

#[allow(clippy::too_many_arguments)]
pub async fn convert(
    ffmpeg: &Path,
    ffprobe: &Path,
    src: &Path,
    dst: &Path,
    target: Format,
    opts: &AvOptions,
    cancel: &CancellationToken,
    mut progress: impl FnMut(f32, String) + Send,
) -> Result<()> {
    let duration = match target {
        Format::Gif | Format::Mp3 | Format::Wav | Format::Flac | Format::Ogg
        | Format::Opus | Format::M4a | Format::Aac => probe_duration(ffprobe, src).await?,
        _ => probe_duration(ffprobe, src).await?,
    };

    if target == Format::Gif {
        return two_pass_gif(ffmpeg, src, dst, duration, opts, cancel, progress).await;
    }

    let is_video_target = target.is_video() || target == Format::Gif;
    let src_is_image = !matches!(target, Format::Gif) && {
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
        matches!(Format::from_extension(ext), Some(f) if f.category() == crate::format::Category::Image)
    };

    let mut args: Vec<String> = vec!["-y".into(), "-hide_banner".into(), "-loglevel".into(), "error".into()];
    if src_is_image && is_video_target {
        // single still image -> short clip
        args.extend(["-loop".into(), "1".into(), "-t".into(), "3".into()]);
    }
    args.push("-i".into());
    args.push(src.display().to_string());

    if target.is_video() {
        args.extend(video_args(target, opts));
        if src_is_audio_only(src) {
            args.push("-vn".into());
        }
    } else if target.is_audio() {
        args.extend(audio_args(target, opts));
    } else if matches!(target, Format::Png | Format::Jpg | Format::Webp | Format::Bmp | Format::Tiff) {
        // poster frame
        args.extend(["-frames:v".into(), "1".into()]);
        if target == Format::Jpg {
            args.extend(["-q:v".into(), "3".into()]);
        }
    } else {
        return Err(Error::Other(format!("ffmpeg pipeline cannot encode {target}")));
    }

    args.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    args.push(dst.display().to_string());

    run(ffmpeg, &args, duration, cancel, &mut progress).await
}

fn src_is_audio_only(src: &Path) -> bool {
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(Format::from_extension(ext), Some(f) if f.is_audio())
}

async fn two_pass_gif(
    ffmpeg: &Path,
    src: &Path,
    dst: &Path,
    duration: Option<f64>,
    _opts: &AvOptions,
    cancel: &CancellationToken,
    mut progress: impl FnMut(f32, String) + Send,
) -> Result<()> {
    let prep = "fps=12,scale=480:-1:flags=lanczos";
    let palette = dst.with_extension("palette.png");

    // pass 1: generate palette
    let args1: Vec<String> = vec![
        "-y".into(), "-hide_banner".into(), "-loglevel".into(), "error".into(),
        "-i".into(), src.display().to_string(),
        "-vf".into(), format!("{prep},palettegen=stats_mode=diff"),
        palette.display().to_string(),
    ];
    progress(0.0, "pass 1/2".into());
    run(ffmpeg, &args1, duration, cancel, &mut |r, n| {
        progress(r * 0.35, n)
    }).await?;

    // pass 2: apply palette
    let args2: Vec<String> = vec![
        "-y".into(), "-hide_banner".into(), "-loglevel".into(), "error".into(),
        "-i".into(), src.display().to_string(),
        "-i".into(), palette.display().to_string(),
        "-filter_complex".into(),
        format!("[0:v]{prep}[v];[v][1:v]paletteuse=dither=bayer:bayer_scale=5[out]"),
        "-map".into(), "[out]".into(),
        dst.display().to_string(),
    ];
    progress(0.35, "pass 2/2".into());
    let res = run(ffmpeg, &args2, duration, cancel, &mut |r, n| {
        progress(0.35 + r * 0.65, n)
    }).await;
    let _ = std::fs::remove_file(&palette);
    res
}

async fn run(
    ffmpeg: &Path,
    args: &[String],
    duration: Option<f64>,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(f32, String) + Send),
) -> Result<()> {
    let mut child = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");

    let (ptx, mut prx) = tokio::sync::mpsc::unbounded_channel::<f32>();
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(&mut stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(v) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = v.parse::<f64>() {
                    let _ = ptx.send((us / 1e6) as f32);
                }
            } else if let Some(v) = line.strip_prefix("out_time_ms=") {
                // older builds emit ms (actually microseconds in modern builds);
                // handled by out_time_us first, this is a fallback
                if let Ok(ms) = v.parse::<f64>() {
                    let _ = ptx.send((ms / 1e6) as f32);
                }
            }
        }
    });

    let err_task = tokio::spawn(async move {
        let mut buf = String::new();
        let mut lines = BufReader::new(&mut stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            buf.push_str(&line);
            buf.push('\n');
            if buf.len() > 16 * 1024 {
                buf.drain(..8 * 1024);
            }
        }
        buf
    });

    let mut last_reported = 0.0f32;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(Error::Cancelled);
            }
            Some(secs) = prx.recv() => {
                if let Some(d) = duration {
                    if d > 0.0 {
                        let ratio = (secs as f64 / d).clamp(0.0, 1.0) as f32;
                        if ratio - last_reported > 0.01 {
                            last_reported = ratio;
                            progress(ratio, String::new());
                        }
                    }
                }
            }
            status = child.wait() => {
                let status = status?;
                reader.abort();
                let err = err_task.await.unwrap_or_default();
                if status.success() {
                    progress(1.0, String::new());
                    return Ok(());
                }
                let tail: String = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev()
                    .collect::<Vec<_>>().join(" | ");
                return Err(Error::ProcessFailed {
                    engine: "ffmpeg".into(),
                    code: status.code().unwrap_or(-1),
                    stderr: tail,
                });
            }
        }
    }
}
