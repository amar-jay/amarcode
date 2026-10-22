//! Native clipboard access and image encoding. Call only on a blocking worker:
//! raw screenshot pixels stay here; the webview receives compressed images.

use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};

use arboard::ImageData;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use serde::Serialize;

const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_IMAGES: usize = 4;
const MAX_IMAGE_DIMENSION: usize = 8192;
const MAX_IMAGE_PIXELS: usize = 40_000_000;

#[derive(Debug, Default, Serialize)]
pub struct PastedFiles {
    images: Vec<PastedImage>,
    fallback_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PastedImage {
    filename: String,
    media_type: &'static str,
    data: String,
}

pub fn read_pasted_files(paths: Vec<String>) -> Result<PastedFiles, String> {
    if !paths.is_empty() {
        return Ok(load_images(paths.into_iter().map(PathBuf::from).collect()));
    }

    // Create, use, and drop the native clipboard on the same blocking worker.
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    let files = clipboard.get().file_list();
    read_native_clipboard(files, || clipboard.get_image())
}

fn read_native_clipboard(
    files: Result<Vec<PathBuf>, arboard::Error>,
    read_image: impl FnOnce() -> Result<ImageData<'static>, arboard::Error>,
) -> Result<PastedFiles, String> {
    if let Ok(paths) = files {
        if !paths.is_empty() {
            // Keep unsupported/missing files as paths rather than substituting
            // another clipboard format for the user's copied files.
            return Ok(load_images(paths));
        }
    }

    // Some platforms cannot expose file lists even when an image is present.
    // Try the image format after an absent, empty, or unsupported file list.
    match read_image() {
        Ok(image) => Ok(PastedFiles {
            images: vec![encode_screenshot(&image)?],
            fallback_paths: Vec::new(),
        }),
        Err(arboard::Error::ContentNotAvailable) => Ok(PastedFiles::default()),
        Err(error) => Err(format!("Unable to read clipboard image: {error}")),
    }
}

fn encode_screenshot(image: &ImageData<'_>) -> Result<PastedImage, String> {
    let pixels = image.width.checked_mul(image.height);
    let bytes = pixels.and_then(|pixels| pixels.checked_mul(4));
    if image.width == 0
        || image.height == 0
        || image.width > MAX_IMAGE_DIMENSION
        || image.height > MAX_IMAGE_DIMENSION
        || pixels.is_none_or(|pixels| pixels > MAX_IMAGE_PIXELS)
        || bytes != Some(image.bytes.len())
    {
        return Err("Clipboard image has invalid or oversized pixel dimensions.".into());
    }

    let mut png = ImageBuffer(Vec::new());
    PngEncoder::new(&mut png)
        .write_image(
            &image.bytes,
            image.width as u32,
            image.height as u32,
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("Unable to encode clipboard image: {error}"))?;
    Ok(PastedImage {
        filename: "clipboard.png".into(),
        media_type: "image/png",
        data: STANDARD.encode(png.0),
    })
}

/// Bound compressed output while encoding, before it can grow into a large
/// allocation or IPC payload. Copied image files use the same byte limit.
struct ImageBuffer(Vec<u8>);

impl Write for ImageBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) as u64 > MAX_IMAGE_BYTES {
            return Err(std::io::Error::other(
                "Clipboard image exceeds the 10 MB limit.",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn load_images(paths: Vec<PathBuf>) -> PastedFiles {
    let mut result = PastedFiles::default();
    for path in paths {
        // Some Linux URI lists use CRLF; arboard splits them on LF only.
        #[cfg(target_os = "linux")]
        let path = PathBuf::from(path.to_string_lossy().trim_end_matches('\r'));
        if result.images.len() < MAX_IMAGES {
            if let Some(image) = load_image(&path) {
                result.images.push(image);
                continue;
            }
        }
        result
            .fallback_paths
            .push(path.to_string_lossy().into_owned());
    }
    result
}

fn load_image(path: &std::path::Path) -> Option<PastedImage> {
    if !path.is_absolute() {
        return None;
    }
    // Check before opening so directories and special files are not read.
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return None;
    }
    let media_type = image_media_type(&bytes)?;
    Some(PastedImage {
        filename: path.file_name()?.to_string_lossy().into_owned(),
        media_type,
        data: STANDARD.encode(bytes),
    })
}

fn image_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn screenshot() -> ImageData<'static> {
        ImageData {
            width: 2,
            height: 1,
            bytes: Cow::Owned(vec![255, 0, 0, 255, 0, 0, 255, 128]),
        }
    }

    #[test]
    fn native_screenshot_fallback_preserves_pixels_and_alpha_in_png() {
        for files in [
            Ok(Vec::new()),
            Err(arboard::Error::ContentNotAvailable),
            Err(arboard::Error::Unknown {
                description: "file lists unsupported".into(),
            }),
        ] {
            let result = read_native_clipboard(files, || Ok(screenshot())).unwrap();
            assert!(result.fallback_paths.is_empty());
            assert_eq!(result.images.len(), 1);
            let image = &result.images[0];
            assert_eq!(image.filename, "clipboard.png");
            assert_eq!(image.media_type, "image/png");
            let png = STANDARD.decode(&image.data).unwrap();
            let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                .unwrap()
                .into_rgba8();
            assert_eq!(decoded.dimensions(), (2, 1));
            assert_eq!(decoded.as_raw().as_slice(), screenshot().bytes.as_ref());
        }
    }

    #[test]
    fn native_file_list_does_not_fall_back_to_a_different_clipboard_format() {
        let path = std::env::temp_dir().join("amarcode-missing-clipboard-image.png");
        let result = read_native_clipboard(Ok(vec![path.clone()]), || {
            panic!("file-manager paste must not read the screenshot format")
        })
        .unwrap();
        assert!(result.images.is_empty());
        assert_eq!(result.fallback_paths, [path.to_string_lossy()]);
    }

    #[test]
    fn native_empty_clipboard_is_empty_but_image_access_errors_are_reported() {
        let result =
            read_native_clipboard(Ok(Vec::new()), || Err(arboard::Error::ContentNotAvailable))
                .unwrap();
        assert!(result.images.is_empty() && result.fallback_paths.is_empty());
        let error = read_native_clipboard(Ok(Vec::new()), || {
            Err(arboard::Error::Unknown {
                description: "clipboard unavailable".into(),
            })
        })
        .unwrap_err();
        assert!(error.contains("clipboard unavailable"));
    }

    #[test]
    fn screenshot_validation_rejects_invalid_buffers_and_oversized_dimensions() {
        for (width, height, length) in [
            (0, 1, 0),
            (1, 0, 0),
            (1, 1, 3),
            (MAX_IMAGE_DIMENSION + 1, 1, 0),
            (MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION, 0),
            (usize::MAX, usize::MAX, 0),
        ] {
            assert!(encode_screenshot(&ImageData {
                width,
                height,
                bytes: Cow::Owned(vec![0; length]),
            })
            .is_err());
        }
    }

    #[test]
    fn png_output_cannot_grow_past_the_attachment_limit() {
        let mut buffer = ImageBuffer(vec![0; MAX_IMAGE_BYTES as usize - 1]);
        assert_eq!(buffer.write(&[1]).unwrap(), 1);
        assert!(buffer.write(&[2]).is_err());
        assert_eq!(buffer.0.len(), MAX_IMAGE_BYTES as usize);
    }

    #[test]
    fn reads_image_content_and_preserves_unattachable_paths() {
        let root =
            std::env::temp_dir().join(format!("amarcode-pasted-files-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let image = root.join("a photo.PNG");
        let text = root.join("not-an-image.png");
        let missing = root.join("missing.jpg");
        let oversized = root.join("oversized.png");
        let png = b"\x89PNG\r\n\x1a\ncontent";
        std::fs::write(&image, png).unwrap();
        std::fs::write(&text, b"plain text with an image extension").unwrap();
        File::create(&oversized)
            .unwrap()
            .set_len(MAX_IMAGE_BYTES + 1)
            .unwrap();

        let result = load_images(vec![
            image,
            text.clone(),
            missing.clone(),
            root.clone(),
            oversized.clone(),
        ]);
        assert_eq!(result.images.len(), 1);
        assert_eq!(result.images[0].filename, "a photo.PNG");
        assert_eq!(result.images[0].media_type, "image/png");
        assert_eq!(STANDARD.decode(&result.images[0].data).unwrap(), png);
        assert_eq!(
            result.fallback_paths,
            [text, missing, root.clone(), oversized]
                .map(|path| path.to_string_lossy().into_owned())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeps_paths_for_images_beyond_the_attachment_limit() {
        let root =
            std::env::temp_dir().join(format!("amarcode-pasted-limit-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = (0..5)
            .map(|n| {
                let path = root.join(format!("{n}.gif"));
                std::fs::write(&path, b"GIF89aimage").unwrap();
                path
            })
            .collect::<Vec<_>>();
        let result = load_images(paths.clone());
        assert_eq!(result.images.len(), MAX_IMAGES);
        assert_eq!(result.fallback_paths, [paths[4].to_string_lossy()]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
