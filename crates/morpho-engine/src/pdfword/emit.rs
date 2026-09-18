//! docx emission: re-flowed blocks -> OOXML via docx-rs.

use std::path::Path;

use docx_rs::*;

use super::layout::{Block, Flow, Heading};
use crate::error::{Error, Result};

const CONTENT_WIDTH_PT: f32 = 468.0; // ~6.5in usable width

pub fn emit_docx(flow: &Flow, dst: &Path) -> Result<()> {
    let mut docx = Docx::new();

    // semantic heading styles; direct run formatting guarantees appearance
    let styles = [
        ("Heading1", "heading 1", 17.0),
        ("Heading2", "heading 2", 14.0),
        ("Heading3", "heading 3", 12.0),
    ];
    for (id, name, pt) in styles {
        docx = docx.add_style(
            Style::new(id, StyleType::Paragraph)
                .name(name)
                .based_on("Normal")
                .size((pt * 2.0) as usize)
                .bold(),
        );
    }

    for (page_idx, page) in flow.pages.iter().enumerate() {
        let mut first_block = true;
        for block in &page.blocks {
            let page_break = page_idx > 0 && first_block;
            first_block = false;
            match block {
                Block::Para(para) => {
                    let mut p = Paragraph::new();
                    if page_break {
                        p = p.page_break_before(true);
                    }
                    match para.heading {
                        Some(Heading::H1) => p = p.style("Heading1"),
                        Some(Heading::H2) => p = p.style("Heading2"),
                        Some(Heading::H3) => p = p.style("Heading3"),
                        None => {}
                    }
                    if para.list {
                        p = p.indent(Some(360), None, None, None);
                    }
                    for run in &para.runs {
                        let mut r = Run::new().add_text(&run.text);
                        let half = (para.size * 2.0).round().max(11.0);
                        r = r.size(half as usize);
                        if run.bold || para.heading.is_some() {
                            r = r.bold();
                        }
                        if run.italic {
                            r = r.italic();
                        }
                        if let Some(color) = para
                            .color
                            .as_deref()
                            .filter(|c| !c.eq_ignore_ascii_case("#000000"))
                        {
                            r = r.color(color.trim_start_matches('#'));
                        }
                        p = p.add_run(r);
                    }
                    docx = docx.add_paragraph(p);
                }
                Block::Image(img) => {
                    let bytes = std::fs::read(&img.path)
                        .map_err(|e| Error::Other(format!("image {}: {e}", img.path.display())))?;
                    let mut w = img.w;
                    let mut h = img.h;
                    if w > CONTENT_WIDTH_PT {
                        h *= CONTENT_WIDTH_PT / w;
                        w = CONTENT_WIDTH_PT;
                    }
                    let pic = Pic::new(&bytes)
                        .size((w * 12700.0) as u32, (h * 12700.0) as u32);
                    let mut p = Paragraph::new()
                        .align(AlignmentType::Center)
                        .add_run(Run::new().add_image(pic));
                    if page_break {
                        p = p.page_break_before(true);
                    }
                    docx = docx.add_paragraph(p);
                }
                Block::Table { rows, .. } => {
                    let table = build_table(rows);
                    docx = docx.add_table(table);
                    docx = docx.add_paragraph(Paragraph::new());
                }
            }
        }
    }

    let file = std::fs::File::create(dst)
        .map_err(|e| Error::Other(format!("create {}: {e}", dst.display())))?;
    docx.build()
        .pack(file)
        .map_err(|e| Error::Other(format!("pack docx: {e}")))?;
    Ok(())
}

fn build_table(rows: &[Vec<String>]) -> Table {
    let border = |pos: TableBorderPosition| {
        TableBorder::new(pos).size(4).color("999999")
    };
    let borders = TableBorders::new()
        .set(border(TableBorderPosition::Top))
        .set(border(TableBorderPosition::Bottom))
        .set(border(TableBorderPosition::Left))
        .set(border(TableBorderPosition::Right))
        .set(border(TableBorderPosition::InsideH))
        .set(border(TableBorderPosition::InsideV));
    let trows: Vec<TableRow> = rows
        .iter()
        .map(|cells| {
            TableRow::new(
                cells
                    .iter()
                    .map(|text| {
                        TableCell::new().add_paragraph(
                            Paragraph::new().add_run(Run::new().add_text(text)),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    Table::new(trows).set_borders(borders)
}
