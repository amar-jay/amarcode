//! File-manager clipboard entries contain paths rather than decoded pixels.

use std::{fs::File, io::Read, path::PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;

const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_IMAGES: usize = 4;

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
    let paths = if paths.is_empty() {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
        match clipboard.get().file_list() {
            Ok(paths) => paths,
            Err(arboard::Error::ContentNotAvailable) => return Ok(PastedFiles::default()),
            Err(error) => return Err(error.to_string()),
        }
    } else {
        paths.into_iter().map(PathBuf::from).collect()
    };
    Ok(load_images(paths))
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
