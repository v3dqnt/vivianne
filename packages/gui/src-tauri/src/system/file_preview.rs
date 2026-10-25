//! Type-aware file preview for the All-files panel and composer attachments.
//!
//! The panel previously called `cmd_read_file_content`, a bare
//! `read_to_string` — so every non-UTF-8 file (every PNG, PDF, archive, and
//! compiled binary in the tree) rendered the same way: the raw error
//! "stream did not contain valid UTF-8". A file browser that can only open
//! text files isn't a file browser.
//!
//! This classifies by extension first and content second, and returns a
//! tagged payload the frontend renders per kind: real images as images, PDFs
//! in a viewer, text as text, and anything else as an honest binary summary
//! rather than mojibake.

use serde::Serialize;

/// Text past this is truncated for preview. Generous — the panel scrolls —
/// but bounded so a multi-hundred-MB log can't be pushed through IPC.
const MAX_TEXT_CHARS: usize = 400_000;

/// Images larger than this get downscaled before base64-ing. A 20MB photo
/// becomes a ~27MB base64 string over IPC for a thumbnail nobody will
/// inspect at full resolution.
const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024;
/// Longest edge after downscaling — comfortably above the panel's width at
/// any zoom, without carrying a camera original.
const DOWNSCALE_MAX_EDGE: u32 = 1600;

/// Bytes of a binary file rendered as a hex dump. Enough to recognise a
/// format from its magic bytes; not enough to be a hex editor.
const HEX_PREVIEW_BYTES: usize = 512;

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "avif"];

/// Formats the webview can play or draw from bytes: video and audio through
/// its own media element, `.glb` through the 3D viewer.
const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "webm", "mov", "ogv"];
const AUDIO_EXTS: &[&str] = &["mp3", "wav", "ogg", "oga", "flac", "m4a", "aac", "opus", "weba"];
const MODEL_EXTS: &[&str] = &["glb", "gltf"];

/// Media past this isn't pushed through IPC as base64: a feature film would
/// stall the app to show a player nobody asked to buffer whole.
const MAX_MEDIA_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilePreview {
    #[serde(rename_all = "camelCase")]
    Text { content: String, truncated: bool, lines: usize },
    #[serde(rename_all = "camelCase")]
    Image { base64: String, media_type: String, width: u32, height: u32, bytes: u64, downscaled: bool },
    /// SVG is an image the browser renders far better from source than from a
    /// rasterised copy, and it's also valid text — so it carries both and the
    /// frontend decides.
    #[serde(rename_all = "camelCase")]
    Svg { source: String },
    #[serde(rename_all = "camelCase")]
    Pdf { base64: String, bytes: u64 },
    /// Video, audio or a 3D model, as bytes for a blob URL. `media` is
    /// `video`, `audio` or `model`.
    #[serde(rename_all = "camelCase")]
    Media { media: &'static str, base64: String, media_type: String, bytes: u64 },
    #[serde(rename_all = "camelCase")]
    Binary { bytes: u64, hex: String, media_type: Option<String>, note: String },
}

fn media_type_for(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "opus" => "audio/opus",
        "weba" => "audio/webm",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "mp3" => "audio/mpeg",
        _ => return None,
    })
}

/// A short, human description for a binary we can't render — better than
/// showing the user a hex dump with no idea what they're looking at.
fn describe_binary(ext: &str) -> String {
    match ext {
        "zip" => "ZIP archive".into(),
        "docx" => "Word document (zipped OOXML) — no text extractor is built in yet".into(),
        "xlsx" => "Excel workbook (zipped OOXML) — no text extractor is built in yet".into(),
        "pptx" => "PowerPoint deck (zipped OOXML) — no text extractor is built in yet".into(),
        "exe" | "dll" | "so" | "dylib" => "Compiled binary".into(),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" => "Video file — too large to preview here (over 64 MB), or a format the webview can't play".into(),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "Audio file — too large to preview here (over 64 MB)".into(),
        "glb" | "gltf" => "3D model — too large to preview here (over 64 MB)".into(),
        "woff" | "woff2" | "ttf" | "otf" => "Font file".into(),
        "" => "Binary file (no extension)".into(),
        other => format!("Binary .{other} file"),
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            // Printable ASCII only; everything else is a dot, same as `xxd`.
            let ascii: String = chunk
                .iter()
                .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                .collect();
            format!("{:08x}  {:<47}  {}", i * 16, hex.join(" "), ascii)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Re-encode an oversized image as PNG at a bounded size.
///
/// Returns `None` on any decode failure — a corrupt or exotic image falls
/// through to the raw-bytes path rather than failing the whole preview.
fn downscale(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(bytes).ok()?;
    let scaled = img.thumbnail(DOWNSCALE_MAX_EDGE, DOWNSCALE_MAX_EDGE);
    let mut out = std::io::Cursor::new(Vec::new());
    scaled.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some((out.into_inner(), scaled.width(), scaled.height()))
}

/// Classify and read a file for display.
///
/// Extension first, content second: a `.png` is treated as an image even if
/// truncated, and a file with no extension is still shown as text when its
/// bytes are valid UTF-8 — which is what most extensionless files in a repo
/// (LICENSE, Makefile, .gitignore) actually are.
#[tauri::command]
pub async fn cmd_read_file_preview(path: String) -> Result<FilePreview, String> {
    let p = std::path::PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("Not a file: {path}"));
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    tokio::task::spawn_blocking(move || {
        let meta = std::fs::metadata(&p).map_err(|e| format!("Couldn't stat file: {e}"))?;
        let size = meta.len();

        if ext == "svg" {
            let source = std::fs::read_to_string(&p).map_err(|e| format!("Couldn't read SVG: {e}"))?;
            return Ok(FilePreview::Svg { source });
        }

        if IMAGE_EXTS.contains(&ext.as_str()) {
            let raw = std::fs::read(&p).map_err(|e| format!("Couldn't read image: {e}"))?;
            let dims = image::load_from_memory(&raw).ok().map(|i| (i.width(), i.height()));
            let (data, width, height, downscaled) = if size > MAX_IMAGE_BYTES {
                match downscale(&raw) {
                    Some((d, w, h)) => (d, w, h, true),
                    None => {
                        let (w, h) = dims.unwrap_or((0, 0));
                        (raw, w, h, false)
                    }
                }
            } else {
                let (w, h) = dims.unwrap_or((0, 0));
                (raw, w, h, false)
            };
            return Ok(FilePreview::Image {
                // A downscale always re-encodes to PNG, so the original
                // extension's media type would be a lie after one.
                media_type: if downscaled {
                    "image/png".to_string()
                } else {
                    media_type_for(&ext).unwrap_or("application/octet-stream").to_string()
                },
                base64: base64_encode(&data),
                width,
                height,
                bytes: size,
                downscaled,
            });
        }

        let media = if VIDEO_EXTS.contains(&ext.as_str()) {
            Some("video")
        } else if AUDIO_EXTS.contains(&ext.as_str()) {
            Some("audio")
        } else if MODEL_EXTS.contains(&ext.as_str()) {
            Some("model")
        } else {
            None
        };
        if let Some(media) = media {
            if size <= MAX_MEDIA_BYTES {
                let raw = std::fs::read(&p).map_err(|e| format!("Couldn't read file: {e}"))?;
                return Ok(FilePreview::Media {
                    media,
                    base64: base64_encode(&raw),
                    media_type: media_type_for(&ext).unwrap_or("application/octet-stream").to_string(),
                    bytes: size,
                });
            }
        }

        if ext == "pdf" {
            let raw = std::fs::read(&p).map_err(|e| format!("Couldn't read PDF: {e}"))?;
            return Ok(FilePreview::Pdf { base64: base64_encode(&raw), bytes: size });
        }

        // Not a known renderable type: try text, fall back to binary. Reading
        // to a String is the check — a file is "text" exactly when it's valid
        // UTF-8, which is a stronger and simpler test than sniffing for NUL
        // bytes in a prefix.
        match std::fs::read_to_string(&p) {
            Ok(content) => {
                let truncated = content.len() > MAX_TEXT_CHARS;
                let lines = content.lines().count();
                Ok(FilePreview::Text {
                    content: if truncated { content.chars().take(MAX_TEXT_CHARS).collect() } else { content },
                    truncated,
                    lines,
                })
            }
            Err(_) => {
                let mut head = vec![0u8; HEX_PREVIEW_BYTES];
                let read = {
                    use std::io::Read;
                    let mut f = std::fs::File::open(&p).map_err(|e| format!("Couldn't open file: {e}"))?;
                    f.read(&mut head).map_err(|e| format!("Couldn't read file: {e}"))?
                };
                head.truncate(read);
                Ok(FilePreview::Binary {
                    bytes: size,
                    hex: hex_dump(&head),
                    media_type: media_type_for(&ext).map(str::to_string),
                    note: describe_binary(&ext),
                })
            }
        }
    })
    .await
    .map_err(|e| format!("preview task panicked: {e}"))?
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_dump_is_offset_hex_ascii() {
        let dump = hex_dump(b"Hi\x00\xffthere");
        assert!(dump.starts_with("00000000  "));
        assert!(dump.contains("48 69 00 ff"), "hex bytes missing: {dump}");
        // Non-printables collapse to dots, printables survive.
        assert!(dump.trim_end().ends_with("Hi..there"), "ascii column wrong: {dump}");
    }

    #[test]
    fn known_extensions_map_to_media_types() {
        assert_eq!(media_type_for("png"), Some("image/png"));
        assert_eq!(media_type_for("pdf"), Some("application/pdf"));
        assert_eq!(media_type_for("qqq"), None);
    }
}
