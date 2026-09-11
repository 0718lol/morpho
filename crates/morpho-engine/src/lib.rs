//! Morpho core conversion engine — pure library shared by the GUI and CLI.

pub mod av;
pub mod docs;
pub mod engines;
pub mod error;
pub mod format;
pub mod image;
pub mod ocr;
pub mod office;
pub mod pdfs;
pub mod queue;
pub mod route;

pub use error::{Error, Result};
pub use format::{Category, Format};
pub use queue::{JobEngine, JobEvent, JobOptions, MatrixEntry};

#[cfg(test)]
mod tests {
    use crate::format::Format;
    use crate::format::Format::*;
    use crate::route::{plan, Pipeline};

    fn pipelines(src: Format, dst: Format) -> Option<Vec<Pipeline>> {
        plan(src, dst).map(|p| p.steps.iter().map(|s| s.pipeline).collect())
    }

    #[test]
    fn routes_native_images() {
        assert_eq!(pipelines(Png, Webp), Some(vec![Pipeline::Ffmpeg]));
        assert_eq!(pipelines(Jpg, Ico), Some(vec![Pipeline::NativeImage]));
        assert_eq!(pipelines(Webp, Png), Some(vec![Pipeline::NativeImage]));
    }

    #[test]
    fn routes_exotic_images_via_ffmpeg() {
        assert_eq!(pipelines(Avif, Png), Some(vec![Pipeline::Ffmpeg]));
        assert_eq!(pipelines(Heic, Webp), Some(vec![Pipeline::Ffmpeg]));
        assert_eq!(pipelines(Heic, Jpg), Some(vec![Pipeline::Ffmpeg]));
    }

    #[test]
    fn routes_av() {
        assert_eq!(pipelines(Mp4, Mkv), Some(vec![Pipeline::Ffmpeg]));
        assert_eq!(pipelines(Mov, Mp3), Some(vec![Pipeline::Ffmpeg]));
        assert_eq!(pipelines(Mp4, Gif), Some(vec![Pipeline::Ffmpeg]));
        assert_eq!(pipelines(Mp4, Png), Some(vec![Pipeline::Ffmpeg])); // poster
        assert_eq!(pipelines(Flac, Mp3), Some(vec![Pipeline::Ffmpeg]));
    }

    #[test]
    fn routes_documents() {
        assert_eq!(pipelines(Docx, Pdf), Some(vec![Pipeline::LibreOffice]));
        assert_eq!(pipelines(Md, Epub), Some(vec![Pipeline::Pandoc]));
        assert_eq!(
            pipelines(Md, Pdf),
            Some(vec![Pipeline::Pandoc, Pipeline::LibreOffice])
        );
        assert_eq!(pipelines(Xlsx, Csv), Some(vec![Pipeline::LibreOffice]));
        assert_eq!(pipelines(Pptx, Pdf), Some(vec![Pipeline::LibreOffice]));
        assert_eq!(pipelines(Epub, Docx), Some(vec![Pipeline::Pandoc]));
    }

    #[test]
    fn routes_pdf() {
        assert_eq!(pipelines(Pdf, Txt), Some(vec![Pipeline::Poppler]));
        assert_eq!(pipelines(Pdf, Png), Some(vec![Pipeline::Poppler]));
        assert_eq!(
            pipelines(Pdf, Docx),
            Some(vec![Pipeline::Poppler, Pipeline::Pandoc])
        );
    }

    #[test]
    fn rejects_impossible() {
        assert!(plan(Mp3, Docx).is_none());
        assert!(plan(Png, Png).is_none());
        assert!(plan(Mp3, Mp4).is_none()); // v1: no waveform render
    }

    #[test]
    fn extension_roundtrip() {
        for f in crate::format::Category::all()
            .iter()
            .flat_map(|c| crate::format::formats_in_category(*c))
        {
            let ext = f.extension();
            assert_eq!(Format::from_extension(ext), Some(f), "roundtrip {ext}");
        }
        assert_eq!(Format::from_extension("jpeg"), Some(Jpg));
        assert_eq!(Format::from_extension("yml"), Some(Yaml));
        assert_eq!(Format::from_extension("heif"), Some(Heic));
    }
}
