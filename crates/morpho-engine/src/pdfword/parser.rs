//! pdftohtml XML parser: turns the positioned-chunk XML into typed data.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{Error, Result};

/// pdftohtml renders at 1.5x zoom by default; normalize back to PDF points.
const ZOOM: f32 = 1.5;

#[derive(Debug, Clone)]
pub struct TextRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone)]
pub struct TextChunk {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// font size in pt (zoom-normalized)
    pub size: f32,
    pub color: Option<String>,
    pub runs: Vec<TextRun>,
}

impl TextChunk {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// x of the right edge in pt.
    pub fn x1(&self) -> f32 {
        self.x + self.w
    }
}

#[derive(Debug, Clone)]
pub struct ImageBlock {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub chunks: Vec<TextChunk>,
    pub images: Vec<ImageBlock>,
}

#[derive(Debug, Default)]
pub struct PdfDoc {
    pub pages: Vec<Page>,
}

struct FontSpec {
    size: f32,
    family: String,
    color: Option<String>,
}

/// Accumulator while inside a `<text>` element: the chunk being built, the
/// text pending for the current inline run, the inline bold/italic toggles,
/// and whether the face is a dingbat/symbol font (list bullets).
struct TextAcc {
    chunk: TextChunk,
    pending: String,
    bold: bool,
    italic: bool,
    bullet_font: bool,
}

impl TextAcc {
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if let Some(last) = self.chunk.runs.last_mut() {
            if last.bold == self.bold && last.italic == self.italic {
                last.text.push_str(&std::mem::take(&mut self.pending));
                return;
            }
        }
        self.chunk.runs.push(TextRun {
            text: std::mem::take(&mut self.pending),
            bold: self.bold,
            italic: self.italic,
        });
    }

    fn finalize(mut self) -> Option<TextChunk> {
        self.flush();
        if self.chunk.runs.iter().all(|r| r.text.trim().is_empty()) {
            if self.bullet_font {
                // dingbat/symbol fonts carry list bullets as unmapped glyphs
                self.chunk.runs = vec![TextRun {
                    text: "\u{2022}".into(),
                    bold: false,
                    italic: false,
                }];
            } else {
                return None;
            }
        }
        Some(self.chunk)
    }
}

pub fn parse(xml: &Path, image_dir: &Path) -> Result<PdfDoc> {
    let mut reader = Reader::from_file(xml)
        .map_err(|e| Error::Other(format!("pdftohtml xml unreadable: {e}")))?;

    let mut doc = PdfDoc::default();
    let mut fonts: HashMap<String, FontSpec> = HashMap::new();
    let mut acc: Option<TextAcc> = None;

    let mut buf = Vec::new();
    loop {
        let ev = reader.read_event_into(&mut buf);
        let empty_tag = matches!(ev, Ok(Event::Empty(_)));
        match ev {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name().as_ref().to_ascii_lowercase();
                let attrs = |key: &str| -> Option<String> {
                    e.attributes().find_map(|a| {
                        let a = a.ok()?;
                        if a.key.as_ref().eq_ignore_ascii_case(key) {
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .ok()
                                .map(|v| v.into_owned())
                        } else {
                            None
                        }
                    })
                };
                match name.as_str() {
                    "page" => {
                        let page = Page {
                            width: attrs("width").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                            height: attrs("height").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                            ..Default::default()
                        };
                        doc.pages.push(page);
                        fonts.clear();
                    }
                    "fontspec" => {
                        let id = attrs("id").unwrap_or_default();
                        fonts.insert(
                            id,
                            FontSpec {
                                size: attrs("size").and_then(|v| v.parse().ok()).unwrap_or(12.0),
                                family: attrs("family").unwrap_or_default(),
                                color: attrs("color"),
                            },
                        );
                    }
                    "text" => {
                        let font_id = attrs("font").unwrap_or_default();
                        let spec = fonts.get(&font_id);
                        let chunk = TextChunk {
                            x: attrs("left").and_then(|v| v.parse().ok()).unwrap_or(0.0) / ZOOM,
                            y: attrs("top").and_then(|v| v.parse().ok()).unwrap_or(0.0) / ZOOM,
                            w: attrs("width").and_then(|v| v.parse().ok()).unwrap_or(0.0) / ZOOM,
                            h: attrs("height").and_then(|v| v.parse().ok()).unwrap_or(0.0) / ZOOM,
                            size: spec.map(|s| s.size).unwrap_or(12.0) / ZOOM,
                            color: spec.and_then(|s| s.color.clone()),
                            runs: Vec::new(),
                        };
                        // bold/italic declared by the font face itself
                        let family =
                            spec.map(|s| s.family.to_ascii_lowercase()).unwrap_or_default();
                        let face_bold = family.contains("bold")
                            || family.contains("black")
                            || family.contains("heavy");
                        let face_italic =
                            family.contains("italic") || family.contains("oblique");
                        // dingbat/symbol fonts carry list bullets as unmapped glyphs
                        let bullet_font =
                            family.contains("symbol") || family.contains("dingbats");
                        acc = Some(TextAcc {
                            chunk,
                            pending: String::new(),
                            bold: face_bold,
                            italic: face_italic,
                            bullet_font,
                        });
                        if empty_tag {
                            if let Some(a) = acc.take() {
                                push_chunk(&mut doc, a.finalize());
                            }
                        }
                    }
                    "b" => {
                        if let Some(a) = acc.as_mut() {
                            a.flush();
                            a.bold = true;
                        }
                    }
                    "i" => {
                        if let Some(a) = acc.as_mut() {
                            a.flush();
                            a.italic = true;
                        }
                    }
                    "image" => {
                        if let Some(page) = doc.pages.last_mut() {
                            let src = attrs("src").unwrap_or_default();
                            let path = if Path::new(&src).is_absolute() {
                                PathBuf::from(&src)
                            } else {
                                image_dir.join(&src)
                            };
                            page.images.push(ImageBlock {
                                x: attrs("left").and_then(|v| v.parse().ok()).unwrap_or(0.0)
                                    / ZOOM,
                                y: attrs("top").and_then(|v| v.parse().ok()).unwrap_or(0.0)
                                    / ZOOM,
                                w: attrs("width").and_then(|v| v.parse().ok()).unwrap_or(0.0)
                                    / ZOOM,
                                h: attrs("height").and_then(|v| v.parse().ok()).unwrap_or(0.0)
                                    / ZOOM,
                                path,
                            });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref t)) => {
                if let Some(a) = acc.as_mut() {
                    let text = t.xml10_content();
                    a.pending.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name().as_ref().to_ascii_lowercase();
                match name.as_str() {
                    "b" => {
                        if let Some(a) = acc.as_mut() {
                            a.flush();
                            a.bold = false;
                        }
                    }
                    "i" => {
                        if let Some(a) = acc.as_mut() {
                            a.flush();
                            a.italic = false;
                        }
                    }
                    "text" => {
                        if let Some(a) = acc.take() {
                            push_chunk(&mut doc, a.finalize());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Other(format!("pdftohtml xml parse: {e}"))),
            _ => {}
        }
        buf.clear();
    }
    Ok(doc)
}

fn push_chunk(doc: &mut PdfDoc, chunk: Option<TextChunk>) {
    if let (Some(page), Some(chunk)) = (doc.pages.last_mut(), chunk) {
        page.chunks.push(chunk);
    }
}
