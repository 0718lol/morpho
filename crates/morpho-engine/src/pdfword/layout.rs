//! Geometric re-flow: positioned text chunks -> lines -> paragraphs/tables.

use tokio_util::sync::CancellationToken;

use super::parser::{PdfDoc, TextChunk, TextRun};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heading {
    H1,
    H2,
    H3,
}

#[derive(Debug, Clone)]
pub struct Para {
    pub runs: Vec<TextRun>,
    /// dominant font size in pt
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Option<String>,
    /// hanging list item (bullet kept in the text)
    pub list: bool,
    pub heading: Option<Heading>,
    /// layout position in pt (for ordering against images/tables)
    pub y: f32,
    /// bottom of the last merged line
    y1: f32,
}

#[derive(Debug, Clone)]
pub struct ImagePara {
    pub path: std::path::PathBuf,
    pub w: f32,
    pub h: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub enum Block {
    Para(Para),
    Image(ImagePara),
    Table { y: f32, rows: Vec<Vec<String>> },
}

#[derive(Debug, Default)]
pub struct PageFlow {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Default)]
pub struct Flow {
    pub pages: Vec<PageFlow>,
}

/// A visual line: chunks sharing a baseline.
struct Line {
    chunks: Vec<TextChunk>,
    y0: f32,
    y1: f32,
    x0: f32,
    size: f32,
}

impl Line {
    fn from(mut chunks: Vec<TextChunk>) -> Line {
        chunks.sort_by(|a, b| a.x.total_cmp(&b.x));
        let y0 = chunks.iter().map(|c| c.y).fold(f32::MAX, f32::min);
        let y1 = chunks.iter().map(|c| c.y + c.h).fold(f32::MIN, f32::max);
        let x0 = chunks.iter().map(|c| c.x).fold(f32::MAX, f32::min);
        let size = chunks.iter().map(|c| c.size).fold(f32::MIN, f32::max);
        Line { chunks, y0, y1, x0, size }
    }

    fn center_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    fn text(&self) -> String {
        self.chunks.iter().map(|c| c.text()).collect()
    }

    fn starts_with_bullet(&self) -> bool {
        self.chunks
            .first()
            .map(|c| is_bullet_text(&c.text()))
            .unwrap_or(false)
    }

    fn runs(&self) -> Vec<TextRun> {
        let mut runs: Vec<TextRun> = Vec::new();
        let mut prev: Option<&TextChunk> = None;
        for chunk in &self.chunks {
            // horizontal gap between chunks -> explicit space (latin words
            // split across style changes would otherwise concatenate)
            if let Some(p) = prev {
                if chunk.x - p.x1() > 1.2 {
                    if let Some(last) = runs.last_mut() {
                        if !last.text.ends_with(' ') && !last.text.is_empty() {
                            last.text.push(' ');
                        }
                    }
                }
            }
            for run in &chunk.runs {
                if run.text.is_empty() {
                    continue;
                }
                runs.push(TextRun {
                    text: run.text.clone(),
                    bold: run.bold,
                    italic: run.italic,
                });
            }
            prev = Some(chunk);
        }
        runs
    }

    fn bold_majority(&self) -> bool {
        let (bold, total) = self
            .chunks
            .iter()
            .map(|c| {
                (
                    c.runs.iter().map(|r| r.bold as usize).sum::<usize>(),
                    c.text().chars().count(),
                )
            })
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        bold * 2 > total
    }
}

/// Group chunks into visual lines by vertical-center proximity.
fn build_lines(mut chunks: Vec<TextChunk>) -> Vec<Line> {
    chunks.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    let mut lines: Vec<Line> = Vec::new();
    let mut group: Vec<TextChunk> = Vec::new();
    for chunk in chunks {
        let cy = chunk.y + chunk.h / 2.0;
        if group.is_empty() {
            group.push(chunk);
            continue;
        }
        let cur = Line::from(group.clone());
        let tol = 0.6 * cur.size.max(chunk.size);
        if (cy - cur.center_y()).abs() <= tol {
            group.push(chunk);
        } else {
            lines.push(Line::from(std::mem::take(&mut group)));
            group.push(chunk);
        }
    }
    if !group.is_empty() {
        lines.push(Line::from(group));
    }
    lines
}

/// Detect grid-aligned runs of lines and turn them into tables.
/// Returns (tables with their first-line y, lines left over).
type TableOut = (f32, Vec<Vec<String>>);
fn extract_tables(lines: Vec<Line>) -> (Vec<TableOut>, Vec<Line>) {
    let mut tables = Vec::new();
    let mut used = vec![false; lines.len()];

    let mut run: Vec<usize> = Vec::new();
    let mut flush = |run: &mut Vec<usize>| {
        if run.len() < 3 {
            run.clear();
            return;
        }
        // columns: x-starts of the first line that recur on most lines
        let cols: Vec<f32> = lines[run[0]].chunks.iter().map(|c| c.x).collect();
        let min_rows = run.len() / 2 + 1;
        let mut aligned: Vec<f32> = cols
            .into_iter()
            .filter(|cx| {
                run.iter()
                    .filter(|&&i| lines[i].chunks.iter().any(|c| (c.x - cx).abs() <= 2.5))
                    .count()
                    >= min_rows
            })
            .collect();
        aligned.sort_by(|a, b| a.total_cmp(b));
        // deduplicate near-identical columns, require sensible gaps
        let mut merged: Vec<f32> = Vec::new();
        for cx in aligned {
            if merged.last().map(|m| cx - m > 15.0).unwrap_or(true) {
                merged.push(cx);
            }
        }
        if merged.len() < 2 {
            run.clear();
            return;
        }
        let y = lines[run[0]].y0;
        let rows: Vec<Vec<String>> = run
            .iter()
            .map(|&i| {
                let mut cells = vec![String::new(); merged.len()];
                for c in &lines[i].chunks {
                    let col = merged
                        .iter()
                        .enumerate()
                        .filter(|&(ci, cx)| {
                            let next = merged.get(ci + 1).copied().unwrap_or(f32::MAX);
                            c.x >= cx - 3.0 && c.x < next - 3.0
                        })
                        .map(|(ci, _)| ci)
                        .next();
                    if let Some(ci) = col {
                        if !cells[ci].is_empty() {
                            cells[ci].push(' ');
                        }
                        cells[ci].push_str(&c.text());
                    }
                }
                cells
            })
            .collect();
        for &i in run.iter() {
            used[i] = true;
        }
        tables.push((y, rows));
        run.clear();
    };

    for (i, line) in lines.iter().enumerate() {
        if line.chunks.len() < 2 {
            flush(&mut run);
            run.push(i);
            continue;
        }
        if let Some(&prev) = run.last() {
            let shared = lines[prev]
                .chunks
                .iter()
                .filter(|c| lines[i].chunks.iter().any(|c2| (c2.x - c.x).abs() <= 2.5))
                .count();
            if shared < 2 {
                flush(&mut run);
            }
        }
        run.push(i);
    }
    flush(&mut run);

    let leftover = lines
        .into_iter()
        .enumerate()
        .filter(|&(i, _)| !used[i])
        .map(|(_, l)| l)
        .collect();
    (tables, leftover)
}

fn is_bullet_text(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('\u{2022}')
        || t.starts_with('\u{25CF}')
        || t.starts_with('\u{25CB}')
        || t.starts_with('\u{25AA}')
        || t.starts_with('\u{00B7}')
        || t.starts_with("* ")
        || t.starts_with("- ")
}

fn is_numbered(text: &str) -> bool {
    let t = text.trim_start();
    let mut chars = t.chars();
    let d1 = chars.next().map(|c| c.is_ascii_digit()).unwrap_or(false);
    let d2 = chars.next().map(|c| c.is_ascii_digit()).unwrap_or(false);
    let sep = chars.next();
    d1 && (sep == Some('.') || sep == Some(')') || (d2 && sep == Some('.')))
}

/// Re-flow parsed pages into paragraph/table/image blocks.
pub fn reflow(
    doc: PdfDoc,
    token: &CancellationToken,
    report: &mut (dyn FnMut(f32, &str) + Send),
) -> Result<Flow> {
    let total = doc.pages.len().max(1);

    // body size: char-weighted mode of line sizes across the document
    let mut size_weights: Vec<(f32, usize)> = Vec::new();
    for page in &doc.pages {
        for line in build_lines(page.chunks.clone()) {
            let chars = line.text().chars().filter(|c| !c.is_whitespace()).count();
            if chars == 0 {
                continue;
            }
            let key = (line.size * 2.0).round() / 2.0;
            if let Some(e) = size_weights.iter_mut().find(|(s, _)| *s == key) {
                e.1 += chars;
            } else {
                size_weights.push((key, chars));
            }
        }
    }
    size_weights.sort_by_key(|(_, w)| std::cmp::Reverse(*w));
    let body_size = size_weights.first().map(|(s, _)| *s).unwrap_or(12.0);

    let mut flow = Flow::default();
    for (idx, page) in doc.pages.into_iter().enumerate() {
        if token.is_cancelled() {
            return Err(Error::Cancelled);
        }
        report(
            0.25 + 0.5 * (idx as f32 + 1.0) / total as f32,
            &format!("page {}/{}", idx + 1, total),
        );
        flow.pages.push(reflow_page(page, body_size));
    }
    Ok(flow)
}

fn reflow_page(page: super::parser::Page, body_size: f32) -> PageFlow {
    let lines = build_lines(page.chunks);
    let (tables, lines) = extract_tables(lines);

    // body left edge: most common line x0 (2pt buckets)
    let mut x0_counts: Vec<(u32, usize)> = Vec::new();
    for l in &lines {
        let key = (l.x0 / 2.0).round() as u32;
        if let Some(e) = x0_counts.iter_mut().find(|(k, _)| *k == key) {
            e.1 += 1;
        } else {
            x0_counts.push((key, 1));
        }
    }
    x0_counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let body_x0 = x0_counts.first().map(|(k, _)| *k as f32 * 2.0).unwrap_or(0.0);

    // merge lines into paragraphs
    let mut paras: Vec<Para> = Vec::new();
    for line in lines {
        let text = line.text();
        let bullet = line.starts_with_bullet();
        let numbered = is_numbered(&text);
        let indented = line.x0 > body_x0 + 18.0;

        let merged = if !bullet {
            if let Some(last) = paras.last_mut() {
                let gap = line.y0 - last.y1;
                let size_ok =
                    (line.size - last.size).abs() <= 0.12 * last.size.max(line.size);
                // 0.8x tolerates generous line spacing (1.5-1.7x) while still
                // splitting at real paragraph gaps (>= 1.2x font size of air)
                let flow_ok = gap < 0.8 * line.size.max(last.size);
                if flow_ok && size_ok && !(numbered && last.list) {
                    merge_line(last, &line);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if !merged {
            paras.push(Para {
                runs: line.runs(),
                size: line.size,
                bold: line.bold_majority(),
                italic: line
                    .chunks
                    .iter()
                    .any(|c| c.runs.iter().any(|r| r.italic)),
                color: line
                    .chunks
                    .iter()
                    .map(|c| c.color.clone())
                    .find(|c| c.as_deref().map(|c| !c.eq_ignore_ascii_case("#000000")).unwrap_or(false))
                    .flatten()
                    .or_else(|| line.chunks.first().and_then(|c| c.color.clone())),
                list: bullet || (numbered && indented),
                heading: None,
                y: line.y0,
                y1: line.y1,
            });
        }
    }

    // heading classification
    for para in &mut paras {
        let chars: usize = para.runs.iter().map(|r| r.text.chars().count()).sum();
        let ratio = para.size / body_size.max(0.1);
        if para.list {
            continue;
        }
        if ratio >= 1.6 {
            para.heading = Some(Heading::H1);
        } else if ratio >= 1.3 {
            para.heading = Some(Heading::H2);
        } else if (ratio >= 1.15 && chars < 120)
            || (para.bold && chars < 80 && ratio >= 1.05)
        {
            para.heading = Some(Heading::H3);
        }
    }

    // interleave paragraphs, images and tables by vertical position
    let mut blocks: Vec<Block> = paras.into_iter().map(Block::Para).collect();
    for img in page.images {
        if img.w < 8.0 || img.h < 8.0 {
            continue; // rule lines / decorations
        }
        blocks.push(Block::Image(ImagePara {
            path: img.path,
            w: img.w,
            h: img.h,
            y: img.y,
        }));
    }
    for (y, rows) in tables {
        blocks.push(Block::Table { y, rows });
    }
    blocks.sort_by_key_order();
    PageFlow { blocks }
}

impl Block {
    pub fn y_of(&self) -> f32 {
        match self {
            Block::Para(p) => p.y,
            Block::Image(i) => i.y,
            Block::Table { y, .. } => *y,
        }
    }
}

trait SortByOrder {
    fn sort_by_key_order(&mut self);
}

impl SortByOrder for Vec<Block> {
    fn sort_by_key_order(&mut self) {
        self.sort_by(|a, b| a.y_of().total_cmp(&b.y_of()));
    }
}

fn merge_line(para: &mut Para, line: &Line) {
    let runs = line.runs();
    // word-wrap boundary: separate two ascii-alphanumeric fragments
    if let (Some(last_run), Some(first)) = (para.runs.last_mut(), runs.first()) {
        if let (Some(lc), Some(fc)) = (last_run.text.chars().last(), first.text.chars().next()) {
            if lc.is_ascii_alphanumeric() && fc.is_ascii_alphanumeric() {
                last_run.text.push(' ');
            }
        }
    }
    para.runs.extend(runs);
    para.y1 = para.y1.max(line.y1);
}
