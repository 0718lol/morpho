//! Job queue: parallel conversions with per-job progress, cancel and events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::av::{self, AvOptions};
use crate::docs;
use crate::engines::Engines;
use crate::error::{Error, Result};
use crate::format::{Category, Format};
use crate::ocr;
use crate::office::LibreOffice;
use crate::pdfs;
use crate::route::{self, Pipeline};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobOptions {
    pub target: Format,
    #[serde(default)]
    pub output_dir: Option<PathBuf>,
    #[serde(default)]
    pub quality: Option<u8>,
    /// "wechat" | "web" | "archive" (video only)
    #[serde(default)]
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    Queued { id: u64, name: String },
    Started { id: u64 },
    Progress { id: u64, ratio: f32, note: String },
    Done { id: u64, output: PathBuf },
    Failed { id: u64, error: String },
    Cancelled { id: u64 },
}

pub struct JobEngine {
    engines: Engines,
    office: Option<LibreOffice>,
    sem: Arc<Semaphore>,
    tx: broadcast::Sender<JobEvent>,
    cancels: Mutex<HashMap<u64, CancellationToken>>,
    next_id: AtomicU64,
}

impl JobEngine {
    pub fn new() -> Arc<Self> {
        let engines = Engines::discover_or_err().expect("engines directory not found; run scripts/fetch_engines.py");
        let office = engines.soffice().map(LibreOffice::new);
        let concurrency = std::thread::available_parallelism()
            .map(|n| n.get().clamp(1, 4))
            .unwrap_or(3);
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self {
            engines,
            office,
            sem: Arc::new(Semaphore::new(concurrency)),
            tx,
            cancels: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.tx.subscribe()
    }

    pub fn engines(&self) -> &Engines {
        &self.engines
    }

    pub fn office(&self) -> Option<&LibreOffice> {
        self.office.as_ref()
    }

    pub fn submit(self: &Arc<Self>, source: PathBuf, opts: JobOptions) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(JobEvent::Queued {
            id,
            name: source.display().to_string(),
        });
        let this = Arc::clone(self);
        tokio::spawn(async move {
            this.run(id, source, opts).await;
        });
        id
    }

    pub fn cancel(&self, id: u64) {
        if let Some(token) = self.cancels.lock().unwrap().get(&id) {
            token.cancel();
        }
    }

    fn emit(&self, ev: JobEvent) {
        let _ = self.tx.send(ev);
    }

    async fn run(self: Arc<Self>, id: u64, source: PathBuf, opts: JobOptions) {
        let token = CancellationToken::new();
        self.cancels.lock().unwrap().insert(id, token.clone());

        self.emit(JobEvent::Started { id });
        let result = self.execute(id, &source, &opts, &token).await;

        self.cancels.lock().unwrap().remove(&id);
        match result {
            Ok(output) => self.emit(JobEvent::Done { id, output }),
            Err(Error::Cancelled) => self.emit(JobEvent::Cancelled { id }),
            Err(e) => self.emit(JobEvent::Failed { id, error: e.to_string() }),
        }
    }

    async fn execute(
        &self,
        id: u64,
        source: &Path,
        opts: &JobOptions,
        token: &CancellationToken,
    ) -> Result<PathBuf> {
        if !source.exists() {
            return Err(Error::InputMissing(source.display().to_string()));
        }
        let src_format = source
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Format::from_extension)
            .ok_or_else(|| {
                Error::Other(format!("unknown input type: {}", source.display()))
            })?;

        let plan = route::plan(src_format, opts.target).ok_or_else(|| {
            Error::Unsupported(src_format.to_string(), opts.target.to_string())
        })?;

        // the chosen output directory may not exist yet
        if let Some(dir) = &opts.output_dir {
            std::fs::create_dir_all(dir)?;
        }

        if token.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let _permit = self.sem.acquire().await;

        // two-stage plans run stage 1 into a temp dir, stage 2 to the final path
        let tmp = tempfile::tempdir()?;
        let weights = step_weights(plan.steps.len());
        let mut current: PathBuf = source.to_path_buf();
        let mut done_weight = 0.0f32;

        for (i, step) in plan.steps.iter().enumerate() {
            if token.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let last = i + 1 == plan.steps.len();
            let target = step.output;
            let dst = if last {
                resolve_output(source, opts, target)
            } else {
                unique_tmp(tmp.path(), i, target)
            };

            let base = done_weight;
            let w = weights[i];
            let mut report = |ratio: f32, note: &str| {
                let r = base + w * ratio.clamp(0.0, 1.0);
                self.emit(JobEvent::Progress {
                    id,
                    ratio: r.clamp(0.0, 1.0),
                    note: note.to_string(),
                });
            };

            self.run_step(&current, &dst, target, opts, step.pipeline, token, &mut report)
                .await?;
            done_weight += w;
            current = dst;
            // multi-page poppler output keeps page suffixes: point at page 1
            if !current.exists() {
                if let Some(first) = first_sibling_page(&current) {
                    current = first;
                }
            }
        }
        Ok(current)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_step(
        &self,
        src: &Path,
        dst: &Path,
        target: Format,
        opts: &JobOptions,
        pipeline: Pipeline,
        token: &CancellationToken,
        report: &mut (dyn FnMut(f32, &str) + Send),
    ) -> Result<()> {
        match pipeline {
            Pipeline::NativeImage => {
                tokio::task::spawn_blocking({
                    let src = src.to_path_buf();
                    let dst = dst.to_path_buf();
                    let q = opts.quality;
                    move || crate::image::convert(&src, &dst, target, q)
                })
                .await
                .map_err(|e| Error::Other(e.to_string()))??;
                report(1.0, "");
            }
            Pipeline::Ffmpeg => {
                let ffmpeg = self.engines.ffmpeg()?;
                let ffprobe = self.engines.ffprobe()?;
                let av_opts = AvOptions {
                    quality: opts.quality,
                    preset: opts.preset.clone(),
                };
                let src = src.to_path_buf();
                let dst = dst.to_path_buf();
                let token2 = token.clone();
                av::convert(&ffmpeg, &ffprobe, &src, &dst, target, &av_opts, &token2, |r, n| {
                    report(r, &n)
                })
                .await?;
            }
            Pipeline::LibreOffice => {
                let lo = self.office.as_ref().ok_or_else(|| {
                    Error::EngineMissing("libreoffice".into(), self.engines.dir.display().to_string())
                })?;
                report(0.3, "LibreOffice");
                // convert into an isolated temp dir so LO's output naming can
                // never clobber the conflict-resolved destination
                let lo_tmp = tempfile::tempdir()?;
                let produced = lo
                    .convert(std::slice::from_ref(&src.to_path_buf()), target, lo_tmp.path())
                    .await?;
                if let Some(p) = produced.first() {
                    if dst.exists() {
                        std::fs::remove_file(dst).ok();
                    }
                    std::fs::rename(p, dst)?;
                }
                report(1.0, "");
            }
            Pipeline::Poppler => match target {
                Format::Txt => {
                    let pdftotext = self
                        .engines
                        .pdftotext()
                        .ok_or_else(|| Error::EngineMissing("poppler".into(), String::new()))?;
                    report(0.2, "poppler");
                    pdfs::pdf_to_text(&pdftotext, src, dst, true).await?;
                    report(1.0, "");
                }
                Format::Png | Format::Jpg => {
                    let pdftoppm = self
                        .engines
                        .pdftoppm()
                        .ok_or_else(|| Error::EngineMissing("poppler".into(), String::new()))?;
                    report(0.2, "poppler");
                    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("page");
                    let pages = pdfs::pdf_to_images(
                        &pdftoppm, src, dst.parent().unwrap_or(Path::new(".")), stem, target, 150,
                    )
                    .await?;
                    if pages.len() == 1 {
                        if pages[0] != dst {
                            std::fs::rename(&pages[0], dst)?;
                        }
                    } else {
                        // multiple pages: dst path is base name without suffix;
                        // rename produced pages to keep poppler naming
                        report(1.0, &format!("{} pages", pages.len()));
                        return Ok(());
                    }
                    report(1.0, "");
                }
                _ => return Err(Error::Other("poppler step target mismatch".into())),
            },
            Pipeline::Pandoc => {
                let pandoc = self
                    .engines
                    .pandoc()
                    .ok_or_else(|| Error::EngineMissing("pandoc".into(), String::new()))?;
                report(0.3, "pandoc");
                let args = docs::convert_args(src, target, dst)
                    .ok_or_else(|| Error::Unsupported("pandoc".into(), target.to_string()))?;
                docs::run(&pandoc, &args).await?;
                report(1.0, "");
            }
            Pipeline::Ocr => {
                let tesseract = self
                    .engines
                    .tesseract()
                    .ok_or_else(|| Error::EngineMissing("tesseract".into(), String::new()))?;
                let tessdata = self
                    .engines
                    .tessdata()
                    .ok_or_else(|| Error::EngineMissing("tessdata".into(), String::new()))?;
                report(0.2, "OCR");
                ocr::ocr(&tesseract, &tessdata, src, dst, &ocr::detect_langs(&tessdata)).await?;
                report(1.0, "");
            }
            Pipeline::Qpdf => {
                return Err(Error::Other("qpdf is command-only (merge/split/encrypt)".into()));
            }
            Pipeline::Copy => {
                std::fs::copy(src, dst)?;
                report(1.0, "");
            }
        }
        Ok(())
    }
}

fn step_weights(n: usize) -> Vec<f32> {
    match n {
        0 | 1 => vec![1.0],
        2 => vec![0.6, 0.4],
        n => vec![1.0 / n as f32; n],
    }
}

fn unique_tmp(dir: &Path, idx: usize, target: Format) -> PathBuf {
    dir.join(format!("stage{idx}.{}", target.extension()))
}

fn first_sibling_page(dst: &Path) -> Option<PathBuf> {
    let parent = dst.parent()?;
    let stem = dst.file_stem()?.to_str()?;
    let ext = dst.extension()?.to_str()?;
    let mut best: Option<(u32, PathBuf)> = None;
    for entry in std::fs::read_dir(parent).ok()?.flatten() {
        let p = entry.path();
        let ps = p.file_stem()?.to_str()?;
        let pe = p.extension()?.to_str()?;
        if pe.eq_ignore_ascii_case(ext) {
            if let Some(suffix) = ps.strip_prefix(&format!("{stem}-")) {
                if let Ok(n) = suffix.parse::<u32>() {
                    if best.as_ref().map(|(b, _)| n < *b).unwrap_or(true) {
                        best = Some((n, p));
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Final output path with conflict-free naming: `name (2).ext`, `name (3).ext`...
pub fn resolve_output(source: &Path, opts: &JobOptions, target: Format) -> PathBuf {
    let dir = opts
        .output_dir
        .clone()
        .unwrap_or_else(|| source.parent().map(|p| p.to_path_buf()).unwrap_or_default());
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = target.extension();
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    candidate
}

/// For the UI: every source format and its supported targets.
#[derive(Debug, Clone, Serialize)]
pub struct MatrixEntry {
    pub format: Format,
    pub category: Category,
    pub targets: Vec<Format>,
}

pub fn ui_matrix() -> Vec<MatrixEntry> {
    let mut out = Vec::new();
    for cat in Category::all() {
        for f in crate::format::formats_in_category(cat) {
            out.push(MatrixEntry {
                format: f,
                category: cat,
                targets: route::targets_for(f),
            });
        }
    }
    out
}
