//! Format matrix: every format Morpho can talk about, its category and extensions.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    // image
    Png,
    Jpg,
    Webp,
    Gif,
    Bmp,
    Tiff,
    Ico,
    Avif,
    Heic,
    // video
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,
    M4v,
    Wmv,
    Flv,
    // audio
    Mp3,
    Wav,
    Flac,
    Ogg,
    Opus,
    M4a,
    Aac,
    // text / documents
    Txt,
    Md,
    Html,
    Json,
    Csv,
    Yaml,
    Rtf,
    Docx,
    Doc,
    Odt,
    Epub,
    // spreadsheets
    Xlsx,
    Xls,
    Ods,
    // slides
    Pptx,
    Ppt,
    Odp,
    // pdf
    Pdf,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Webp => "webp",
            Format::Gif => "gif",
            Format::Bmp => "bmp",
            Format::Tiff => "tiff",
            Format::Ico => "ico",
            Format::Avif => "avif",
            Format::Heic => "heic",
            Format::Mp4 => "mp4",
            Format::Mkv => "mkv",
            Format::Webm => "webm",
            Format::Mov => "mov",
            Format::Avi => "avi",
            Format::M4v => "m4v",
            Format::Wmv => "wmv",
            Format::Flv => "flv",
            Format::Mp3 => "mp3",
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Ogg => "ogg",
            Format::Opus => "opus",
            Format::M4a => "m4a",
            Format::Aac => "aac",
            Format::Txt => "txt",
            Format::Md => "md",
            Format::Html => "html",
            Format::Json => "json",
            Format::Csv => "csv",
            Format::Yaml => "yaml",
            Format::Rtf => "rtf",
            Format::Docx => "docx",
            Format::Doc => "doc",
            Format::Odt => "odt",
            Format::Epub => "epub",
            Format::Xlsx => "xlsx",
            Format::Xls => "xls",
            Format::Ods => "ods",
            Format::Pptx => "pptx",
            Format::Ppt => "ppt",
            Format::Odp => "odp",
            Format::Pdf => "pdf",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Format> {
        let ext = ext.to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => Format::Png,
            "jpg" | "jpeg" | "jpe" => Format::Jpg,
            "webp" => Format::Webp,
            "gif" => Format::Gif,
            "bmp" | "dib" => Format::Bmp,
            "tif" | "tiff" => Format::Tiff,
            "ico" => Format::Ico,
            "avif" => Format::Avif,
            "heic" | "heif" => Format::Heic,
            "mp4" => Format::Mp4,
            "mkv" => Format::Mkv,
            "webm" => Format::Webm,
            "mov" => Format::Mov,
            "avi" => Format::Avi,
            "m4v" => Format::M4v,
            "wmv" => Format::Wmv,
            "flv" => Format::Flv,
            "mp3" => Format::Mp3,
            "wav" => Format::Wav,
            "flac" => Format::Flac,
            "ogg" | "oga" => Format::Ogg,
            "opus" => Format::Opus,
            "m4a" | "m4b" => Format::M4a,
            "aac" => Format::Aac,
            "txt" | "log" | "text" => Format::Txt,
            "md" | "markdown" => Format::Md,
            "html" | "htm" | "xhtml" => Format::Html,
            "json" => Format::Json,
            "csv" | "tsv" => Format::Csv,
            "yaml" | "yml" => Format::Yaml,
            "rtf" => Format::Rtf,
            "docx" => Format::Docx,
            "doc" => Format::Doc,
            "odt" => Format::Odt,
            "epub" => Format::Epub,
            "xlsx" | "xlsm" => Format::Xlsx,
            "xls" | "et" => Format::Xls,
            "ods" => Format::Ods,
            "pptx" => Format::Pptx,
            "ppt" | "dps" => Format::Ppt,
            "odp" => Format::Odp,
            "pdf" => Format::Pdf,
            _ => return None,
        })
    }

    pub fn category(self) -> Category {
        use Format::*;
        match self {
            Png | Jpg | Webp | Bmp | Tiff | Ico | Avif | Heic => Category::Image,
            // Gif is classified as an image but the router also allows gif as a
            // video source/target through ffmpeg.
            Gif => Category::Image,
            Mp4 | Mkv | Webm | Mov | Avi | M4v | Wmv | Flv => Category::Video,
            Mp3 | Wav | Flac | Ogg | Opus | M4a | Aac => Category::Audio,
            Txt | Md | Html | Json | Yaml => Category::Text,
            Csv => Category::Text,
            Rtf | Docx | Doc | Odt | Epub => Category::Document,
            Xlsx | Xls | Ods => Category::Sheet,
            Pptx | Ppt | Odp => Category::Slide,
            Pdf => Category::Pdf,
        }
    }

    /// Formats readable by the native `image` crate (no sidecar needed).
    pub fn is_native_image(self) -> bool {
        matches!(
            self,
            Format::Png
                | Format::Jpg
                | Format::Webp
                | Format::Gif
                | Format::Bmp
                | Format::Tiff
                | Format::Ico
        )
    }

    /// Formats the native `image` crate can ENCODE with meaningful quality.
    /// (webp/avif encoding is routed through ffmpeg for real quality control.)
    pub fn is_native_encode(self) -> bool {
        matches!(
            self,
            Format::Png | Format::Jpg | Format::Gif | Format::Bmp | Format::Tiff | Format::Ico
        )
    }

    pub fn is_video(self) -> bool {
        matches!(
            self,
            Format::Mp4 | Format::Mkv | Format::Webm | Format::Mov | Format::Avi
                | Format::M4v | Format::Wmv | Format::Flv
        )
    }

    pub fn is_audio(self) -> bool {
        matches!(
            self,
            Format::Mp3 | Format::Wav | Format::Flac | Format::Ogg | Format::Opus
                | Format::M4a | Format::Aac
        )
    }

    pub fn is_office_doc(self) -> bool {
        matches!(
            self,
            Format::Rtf | Format::Docx | Format::Doc | Format::Odt | Format::Epub
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Image,
    Video,
    Audio,
    Text,
    Document,
    Sheet,
    Slide,
    Pdf,
}

impl Category {
    pub fn all() -> [Category; 8] {
        [
            Category::Image,
            Category::Video,
            Category::Audio,
            Category::Text,
            Category::Document,
            Category::Sheet,
            Category::Slide,
            Category::Pdf,
        ]
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.extension())
    }
}

impl FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let lower = s.trim().trim_start_matches('.').to_ascii_lowercase();
        Format::from_extension(&lower).ok_or_else(|| format!("unknown format: {s}"))
    }
}

pub fn formats_in_category(cat: Category) -> Vec<Format> {
    use Format::*;
    match cat {
        Category::Image => vec![Png, Jpg, Webp, Avif, Gif, Bmp, Tiff, Ico, Heic],
        Category::Video => vec![Mp4, Mkv, Webm, Mov, Avi, M4v, Wmv, Flv],
        Category::Audio => vec![Mp3, Wav, Flac, Ogg, Opus, M4a, Aac],
        Category::Text => vec![Txt, Md, Html, Csv, Json, Yaml],
        Category::Document => vec![Docx, Doc, Odt, Rtf, Epub],
        Category::Sheet => vec![Xlsx, Xls, Ods],
        Category::Slide => vec![Pptx, Ppt, Odp],
        Category::Pdf => vec![Pdf],
    }
}

/// Every format, deterministic order (CLI `formats` listing).
pub fn all_in_order() -> Vec<Format> {
    Category::all()
        .iter()
        .flat_map(|c| formats_in_category(*c))
        .collect()
}
