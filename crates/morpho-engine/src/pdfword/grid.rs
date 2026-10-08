//! Table grid detection from the rendered page: row and column pixel
//! scans find ruling lines; text chunks falling between the lines become
//! cells, and a chunk spanning several columns marks a merged cell.

use std::path::Path;

use image::{DynamicImage, ImageReader};

use super::parser::TextChunk;
use crate::error::{Error, Result};

/// One detected table: ruling line coordinates in PDF points (page space).
#[derive(Debug, Clone)]
pub struct GridTable {
    /// y of each horizontal ruling line, ascending
    pub ys: Vec<f32>,
    /// x of each vertical ruling line, ascending
    pub xs: Vec<f32>,
}

/// A cell produced by grid slicing: text plus the number of columns it spans.
#[derive(Debug, Clone)]
pub struct GridCell {
    pub text: String,
    pub span: usize,
}

/// Border lines render light-gray in some generators; anything below this
/// counts as line ink (pure white paper is 255).
const DARK: u8 = 245;
const MIN_H_LINES: usize = 3;
const MIN_V_LINES: usize = 2;

/// Longest contiguous run of dark pixels in a byte slice: (start, len).
fn longest_dark_run(px: &[u8]) -> (usize, usize) {
    let mut best = (0usize, 0usize);
    let mut start: Option<usize> = None;
    for (i, &p) in px.iter().enumerate() {
        if p < DARK {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            if i - s > best.1 {
                best = (s, i - s);
            }
        }
    }
    if let Some(s) = start {
        if px.len() - s > best.1 {
            best = (s, px.len() - s);
        }
    }
    best
}

/// Collapse consecutive indices into single line midpoints.
fn collapse(idx: Vec<usize>) -> Vec<f32> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && idx[j + 1] == idx[j] + 1 {
            j += 1;
        }
        out.push(((idx[i] + idx[j]) / 2) as f32);
        i = j + 1;
    }
    out
}

fn detect_page_grid(img: &DynamicImage) -> Option<GridTable> {
    let gray = img.to_luma8();
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    if w < 60 || h < 60 {
        return None;
    }
    let px = gray.as_raw();
    // Horizontal ruling lines: a pixel row whose LONGEST contiguous dark run
    // covers a good part of the page width. Text rows also accumulate dark
    // pixels but break into many short word runs, so total density alone
    // cannot separate them from real lines; run length can.
    // Adaptive horizontal threshold: the longest dark run on the page is
    // taken as the primary ruling line, and rows with >= 40% of it qualify.
    // A fixed page-width fraction fails for narrow tables.
    let runs: Vec<usize> = (0..h)
        .map(|y| longest_dark_run(&px[y * w..(y + 1) * w]).1)
        .collect();
    let max_run = runs.iter().copied().max().unwrap_or(0);
    let min_run = max_run * 40 / 100;
    let hrows: Vec<usize> = runs
        .into_iter()
        .enumerate()
        .filter(|(_, r)| *r >= min_run && *r > 8)
        .map(|(y, _)| y)
        .collect();
    let ys_px = collapse(hrows);
    if ys_px.len() < MIN_H_LINES {
        return None;
    }
    // Vertical ruling lines, constrained to the band between the first and
    // last horizontal lines: they must span most of the table height.
    let y_top = ys_px[0] as usize;
    let y_bot = *ys_px.last().unwrap() as usize;
    if y_bot <= y_top + 20 {
        return None;
    }
    let min_col = (y_bot - y_top) * 70 / 100;
    let mut vcols: Vec<usize> = Vec::new();
    for x in 0..w {
        let col: Vec<u8> = (y_top..y_bot).map(|y| px[y * w + x]).collect();
        if longest_dark_run(&col).1 >= min_col {
            vcols.push(x);
        }
    }
    let xs_px = collapse(vcols);
    if xs_px.len() < MIN_V_LINES {
        return None;
    }
    let scale = 72.0 / 100.0; // rendered at ~100 dpi, back to PDF points
    Some(GridTable {
        ys: ys_px.iter().map(|v| v * scale).collect(),
        xs: xs_px.iter().map(|v| v * scale).collect(),
    })
}

/// Render the pdf at ~100 dpi and detect ruling-line grids page by page.
/// Returns ONE entry per page (None where no grid was detected) so callers
/// can index by page number — a compact list would shift grids onto the
/// wrong pages whenever an earlier page has no table.
pub async fn detect_grids(
    pdftoppm: &Path,
    src: &Path,
) -> Result<Vec<Option<GridTable>>> {
    let tmp = tempfile::tempdir()?;
    let pages = crate::pdfs::pdf_to_images(
        pdftoppm,
        src,
        tmp.path(),
        "grid",
        crate::format::Format::Png,
        100,
    )
    .await?;
    let mut out = Vec::with_capacity(pages.len());
    for p in &pages {
        let img = ImageReader::open(p)
            .map_err(|e| Error::Other(format!("grid render open: {e}")))?
            .decode()
            .map_err(|e| Error::Other(format!("grid render decode: {e}")))?;
        out.push(detect_page_grid(&img));
    }
    Ok(out)
}

/// Slice text chunks into grid cells. A chunk spanning more than one column
/// band becomes a merged cell (span > 1).
pub fn slice_chunks(grid: &GridTable, chunks: &[TextChunk]) -> Vec<Vec<GridCell>> {
    let ncols = grid.xs.len() - 1;
    let mut rows: Vec<Vec<GridCell>> = Vec::new();
    let mut row_cells: Vec<GridCell> = Vec::new();
    let mut cur_row: Option<usize> = None;

    let mut sorted: Vec<&TextChunk> = chunks.iter().collect();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));

    for c in sorted {
        let cy = c.y + c.h / 2.0;
        let ri = match grid.ys.iter().enumerate().find(|(i, y)| {
            cy >= **y && (i + 1 >= grid.ys.len() || cy < grid.ys[i + 1])
        }) {
            Some((i, _)) => i,
            None => continue, // outside the ruled region
        };
        if cur_row != Some(ri) {
            if !row_cells.is_empty() {
                let filled: usize = row_cells.iter().map(|c| c.span).sum();
                for _ in filled..ncols {
                    row_cells.push(GridCell { text: String::new(), span: 1 });
                }
                rows.push(std::mem::take(&mut row_cells));
            }
            cur_row = Some(ri);
        }
        let x0 = c.x;
        let x1 = c.x + c.w;
        let covered: Vec<usize> = (0..ncols)
            .filter(|&ci| x1 > grid.xs[ci] + 1.0 && x0 < grid.xs[ci + 1] - 1.0)
            .collect();
        if covered.is_empty() {
            continue;
        }
        let first = covered[0];
        let filled: usize = row_cells.iter().map(|c| c.span).sum();
        if first < filled {
            // same cell as the previous chunk: append the text
            if let Some(last) = row_cells.last_mut() {
                if !last.text.is_empty() {
                    last.text.push_str(" ");
                }
                last.text.push_str(&c.text());
            }
            continue;
        }
        for _ in filled..first {
            row_cells.push(GridCell { text: String::new(), span: 1 });
        }
        row_cells.push(GridCell { text: c.text(), span: covered.len() });
    }
    if !row_cells.is_empty() {
        let filled: usize = row_cells.iter().map(|c| c.span).sum();
        for _ in filled..ncols {
            row_cells.push(GridCell { text: String::new(), span: 1 });
        }
        rows.push(row_cells);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::super::parser::{TextChunk, TextRun};
    use super::*;

    fn chunk(x: f32, y: f32, w: f32, h: f32, text: &str) -> TextChunk {
        TextChunk {
            x,
            y,
            w,
            h,
            size: 10.0,
            color: None,
            runs: vec![TextRun {
                text: text.to_string(),
                bold: false,
                italic: false,
            }],
        }
    }

    #[test]
    fn slices_cells_and_merged_span() {
        let grid = GridTable {
            ys: vec![100.0, 120.0, 140.0],
            xs: vec![50.0, 150.0, 250.0, 350.0],
        };
        let chunks = vec![
            chunk(60.0, 105.0, 260.0, 10.0, "Merged Header"),
            chunk(60.0, 125.0, 60.0, 10.0, "Q1"),
            chunk(160.0, 125.0, 60.0, 10.0, "120"),
            chunk(260.0, 125.0, 60.0, 10.0, "good"),
        ];
        let rows = slice_chunks(&grid, &chunks);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].span, 3);
        assert_eq!(rows[0][0].text, "Merged Header");
        assert_eq!(rows[1].len(), 3);
        assert_eq!(rows[1][0].text, "Q1");
        assert_eq!(rows[1][1].text, "120");
        assert_eq!(rows[1][2].text, "good");
    }

    #[test]
    fn ignores_chunks_outside_grid() {
        let grid = GridTable {
            ys: vec![100.0, 120.0],
            xs: vec![50.0, 150.0],
        };
        let chunks = vec![
            chunk(60.0, 105.0, 40.0, 10.0, "in"),
            chunk(10.0, 105.0, 30.0, 10.0, "left-margin"),
        ];
        let rows = slice_chunks(&grid, &chunks);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 1);
        assert_eq!(rows[0][0].text, "in");
    }
}
