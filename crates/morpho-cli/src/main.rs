use std::path::PathBuf;

use clap::{Parser, Subcommand};
use morpho_engine::format::Format;
use morpho_engine::queue::{JobEngine, JobEvent, JobOptions};
use morpho_engine::route;

#[derive(Parser)]
#[command(
    name = "morpho",
    version,
    about = "🦋 Morpho — everything converts. Local, fast, private.",
    after_help = "Examples:\n  morpho convert *.png --to webp\n  morpho convert clip.mov --to mp4 --preset web\n  morpho formats docx\n  morpho pdf merge a.pdf b.pdf -o merged.pdf"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Batch-convert files (glob patterns allowed by your shell)
    Convert {
        /// input files
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// target format, e.g. webp, mp4, pdf, docx
        #[arg(short = 't', long = "to")]
        to: String,
        /// output directory (default: alongside each file)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// quality 1-100
        #[arg(short, long)]
        quality: Option<u8>,
        /// video preset: wechat | web | archive
        #[arg(short, long)]
        preset: Option<String>,
    },
    /// Show supported conversion targets (all, or for one source format)
    Formats {
        /// source format extension, e.g. docx
        source: Option<String>,
    },
    /// Show bundled engine availability
    Engines,
    /// PDF tools (merge / split / encrypt / decrypt)
    Pdf {
        #[command(subcommand)]
        op: PdfOp,
    },
}

#[derive(Subcommand)]
enum PdfOp {
    /// merge pdfs into one
    Merge {
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// split into one file per page
    Split {
        input: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// encrypt with AES-256
    Encrypt {
        input: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        #[arg(short = 'p', long)]
        password: String,
    },
    /// remove encryption
    Decrypt {
        input: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        #[arg(short = 'p', long)]
        password: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli.command).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Command) -> Result<(), Box<dyn std::error::Error>> {
    match cli {
        Command::Convert { files, to, out, quality, preset } => {
            let target: Format = to.parse().map_err(|e: String| -> Box<dyn std::error::Error> { e.into() })?;
            let engine = JobEngine::new();
            let mut rx = engine.subscribe();

            let mut ids = Vec::new();
            for f in files {
                let opts = JobOptions {
                    target,
                    output_dir: out.clone(),
                    quality,
                    preset: preset.clone(),
                };
                ids.push((engine.submit(f, opts), target));
            }

            let total = ids.len();
            let mut done = 0usize;
            while done < total {
                if let Ok(ev) = rx.recv().await {
                    match ev {
                        JobEvent::Started { id } => {
                            println!("[{id}] started");
                        }
                        JobEvent::Progress { id, ratio, note } => {
                            print!("\r[{id}] {:>3.0}%{}", ratio * 100.0, if note.is_empty() { String::new() } else { format!(" ({note})") });
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                        JobEvent::Done { id, output } => {
                            done += 1;
                            println!("\r[{id}] done     -> {}", output.display());
                        }
                        JobEvent::Failed { id, error } => {
                            done += 1;
                            println!("\r[{id}] FAILED: {error}");
                        }
                        JobEvent::Cancelled { id } => {
                            done += 1;
                            println!("\r[{id}] cancelled");
                        }
                        JobEvent::Queued { .. } => {}
                    }
                }
            }
            let _ = ids;
            Ok(())
        }
        Command::Formats { source } => match source {
            Some(src) => {
                let f: Format = src.parse().map_err(|e: String| -> Box<dyn std::error::Error> { e.into() })?;
                let targets = route::targets_for(f);
                let list: Vec<String> = targets.iter().map(|t| t.display_name().to_string()).collect();
                println!("{src} -> {}", list.join(", "));
                Ok(())
            }
            None => {
                for f in morpho_engine::format::all_in_order() {
                    let targets = route::targets_for(f);
                    let list: Vec<String> = targets.iter().map(|t| t.display_name().to_string()).collect();
                    println!("{:>5} -> {}", f.display_name(), list.join(", "));
                }
                Ok(())
            }
        },
        Command::Engines => {
            let engines = morpho_engine::engines::Engines::discover_or_err()?;
            for (name, ok) in engines.status() {
                println!("{name:>12}: {}", if ok { "✔" } else { "missing" });
            }
            Ok(())
        }
        Command::Pdf { op } => run_pdf(op).await,
    }
}

async fn run_pdf(op: PdfOp) -> Result<(), Box<dyn std::error::Error>> {
    let engines = morpho_engine::engines::Engines::discover_or_err()?;
    let qpdf = engines
        .qpdf()
        .ok_or("qpdf engine missing; run scripts/fetch_engines.py --only qpdf")?;
    match op {
        PdfOp::Merge { inputs, out } => {
            morpho_engine::pdfs::merge(&qpdf, &inputs, &out).await?;
            println!("merged {} files -> {}", inputs.len(), out.display());
        }
        PdfOp::Split { input, out } => {
            std::fs::create_dir_all(&out)?;
            let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("page");
            let pages = morpho_engine::pdfs::split(&qpdf, &input, &out, stem).await?;
            println!("split into {} pages in {}", pages.len(), out.display());
        }
        PdfOp::Encrypt { input, out, password } => {
            morpho_engine::pdfs::encrypt(&qpdf, &input, &out, &password, None).await?;
            println!("encrypted -> {}", out.display());
        }
        PdfOp::Decrypt { input, out, password } => {
            morpho_engine::pdfs::decrypt(&qpdf, &input, &out, password.as_deref()).await?;
            println!("decrypted -> {}", out.display());
        }
    }
    Ok(())
}
