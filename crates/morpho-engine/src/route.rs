//! Routing: turn (source format, target format) into a chain of pipeline steps.

use crate::format::{Category, Format};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pipeline {
    /// pure-Rust image crate
    NativeImage,
    /// ffmpeg / ffprobe sidecar
    Ffmpeg,
    /// libreoffice headless
    LibreOffice,
    /// pandoc sidecar
    Pandoc,
    /// pdftotext / pdftoppm
    Poppler,
    /// tesseract OCR
    Ocr,
    /// pdf -> searchable pdf: rasterize pages, OCR each into a text-layer
    /// pdf, merge (single step because pages fan out and recombine)
    OcrPdf,
    /// pdf -> txt via per-page OCR (scanned documents with no text layer)
    OcrText,
    /// qpdf (merge/split/encrypt handled by dedicated commands, not routing)
    Qpdf,
    /// self-developed PDF -> Word layout engine (pdftohtml + geometric re-flow)
    PdfWord,
    /// plain file copy (text-ish formats with no transform)
    Copy,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Step {
    pub pipeline: Pipeline,
    /// format this step produces
    pub output: Format,
}

/// A conversion plan = ordered steps. Last step's output == requested target.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    pub steps: Vec<Step>,
}

fn s(p: Pipeline, out: Format) -> Step {
    Step { pipeline: p, output: out }
}

/// Compute the pipeline chain for a conversion, or None if unsupported.
pub fn plan(src: Format, dst: Format) -> Option<Plan> {
    if src == dst {
        return None;
    }
    use Format::*;

    // ---------- from images ----------
    if src.category() == Category::Image && (src.is_native_image() || matches!(src, Avif | Heic)) {
        if dst.category() == Category::Image {
            // webp/avif targets always go through ffmpeg (real quality control);
            // heic has no bundled encoder
            if matches!(dst, Webp | Avif) {
                return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, dst)] });
            }
            if dst == Heic {
                return None;
            }
            // avif/heic decode is not native -> let ffmpeg do decode + encode
            if src == Webp || src.is_native_encode() {
                return Some(Plan { steps: vec![s(Pipeline::NativeImage, dst)] });
            }
            return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, dst)] });
        }
        if dst == Pdf {
            return Some(Plan { steps: vec![s(Pipeline::LibreOffice, Pdf)] });
        }
        if dst == Txt && src.is_native_image() {
            return Some(Plan { steps: vec![s(Pipeline::Ocr, Txt)] });
        }
        if dst == SearchablePdf && src.is_native_image() {
            // tesseract reads the raster directly; one step, no re-encode
            return Some(Plan { steps: vec![s(Pipeline::Ocr, SearchablePdf)] });
        }
        if dst == Docx && src.is_native_image() {
            // editable Word from a scan: OCR text -> pandoc docx
            return Some(Plan {
                steps: vec![s(Pipeline::Ocr, Txt), s(Pipeline::Pandoc, Docx)],
            });
        }
        if dst.is_video() && (src == Gif || src.is_native_image()) {
            // gif/native image -> video (slideshow of a single image is odd but works)
            return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, dst)] });
        }
        return None;
    }

    // ---------- from video ----------
    if src.is_video() {
        if dst.is_video() {
            return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, dst)] });
        }
        if dst.is_audio() {
            return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, dst)] });
        }
        if dst == Gif {
            return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, Gif)] });
        }
        if matches!(dst, Png | Jpg | Webp | Bmp | Tiff) {
            // poster frame (first frame)
            return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, dst)] });
        }
        return None;
    }

    // ---------- from audio ----------
    if src.is_audio() {
        if dst.is_audio() {
            return Some(Plan { steps: vec![s(Pipeline::Ffmpeg, dst)] });
        }
        return None;
    }

    // ---------- from pdf (SearchablePdf behaves like Pdf as a source) ----------
    if matches!(src, Pdf | SearchablePdf) {
        match dst {
            SearchablePdf => {
                return Some(Plan { steps: vec![s(Pipeline::OcrPdf, SearchablePdf)] })
            }
            Txt => return Some(Plan { steps: vec![s(Pipeline::Poppler, Txt)] }),
            Png | Jpg => return Some(Plan { steps: vec![s(Pipeline::Poppler, dst)] }),
            // layout-preserving word first; the docx then feeds odt/rtf/html/md
            Docx => return Some(Plan { steps: vec![s(Pipeline::PdfWord, Docx)] }),
            Odt | Rtf => {
                return Some(Plan {
                    steps: vec![s(Pipeline::PdfWord, Docx), s(Pipeline::LibreOffice, dst)],
                })
            }
            Html => {
                return Some(Plan {
                    steps: vec![s(Pipeline::PdfWord, Docx), s(Pipeline::LibreOffice, Html)],
                })
            }
            Md => {
                return Some(Plan {
                    steps: vec![s(Pipeline::PdfWord, Docx), s(Pipeline::Pandoc, Md)],
                })
            }
            _ => return None,
        }
    }

    // ---------- text / documents / sheets / slides ----------
    let textish_src = matches!(
        src.category(),
        Category::Text | Category::Document | Category::Sheet | Category::Slide
    );
    if textish_src {
        // pandoc-first paths (md/html/txt sources, rich targets)
        let pandoc_src = matches!(src, Md | Html | Txt | Rtf | Docx | Odt | Epub);
        if dst == Pdf {
            // md has no direct LO import; go md -> docx -> pdf
            if matches!(src, Md | Epub) {
                return Some(Plan {
                    steps: vec![s(Pipeline::Pandoc, Docx), s(Pipeline::LibreOffice, Pdf)],
                });
            }
            if pandoc_src || matches!(src.category(), Category::Sheet | Category::Slide)
                || matches!(src, Doc | Csv)
            {
                return Some(Plan { steps: vec![s(Pipeline::LibreOffice, Pdf)] });
            }
            return None;
        }
        if pandoc_src && matches!(dst, Md | Html | Epub | Docx | Odt | Rtf | Txt) {
            return Some(Plan { steps: vec![s(Pipeline::Pandoc, dst)] });
        }
        // libreoffice handles the rest of the office/sheet/slide matrix
        let lo_target = matches!(
            dst,
            Docx | Doc | Odt | Rtf | Txt | Html | Csv | Xlsx | Xls | Ods | Pptx | Ppt | Odp | Epub
        );
        if lo_target {
            return Some(Plan { steps: vec![s(Pipeline::LibreOffice, dst)] });
        }
        // json/yaml/csv passthrough for text families
        if matches!(src.category(), Category::Text)
            && matches!(dst.category(), Category::Text)
        {
            return Some(Plan { steps: vec![s(Pipeline::Copy, dst)] });
        }
        return None;
    }

    None
}

/// All targets supported from `src`, grouped for the UI.
pub fn targets_for(src: Format) -> Vec<Format> {
    crate::format::Category::all()
        .iter()
        .flat_map(|c| crate::format::formats_in_category(*c))
        .filter(|f| plan(src, *f).is_some())
        .collect()
}
