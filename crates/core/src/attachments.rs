//! Bounded preparation of local files for a headless prompt.
use crate::{Capabilities, Error, PromptContent, Result};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnsupportedImage {
    #[default]
    Reject,
    /// Include an absolute path as text for a provider that can read local files.
    PathReference,
}
#[derive(Clone, Debug)]
pub struct AttachmentOptions {
    pub max_image_bytes: usize,
    pub max_text_bytes: usize,
    pub max_total_bytes: usize,
    pub images_supported: bool,
    pub unsupported_image: UnsupportedImage,
    /// When set, canonical attachment paths must remain inside this directory.
    pub root: Option<PathBuf>,
}
impl Default for AttachmentOptions {
    fn default() -> Self {
        Self {
            max_image_bytes: 8 * 1024 * 1024,
            max_text_bytes: 1024 * 1024,
            max_total_bytes: 16 * 1024 * 1024,
            images_supported: false,
            unsupported_image: UnsupportedImage::Reject,
            root: None,
        }
    }
}
impl AttachmentOptions {
    pub fn for_capabilities(capabilities: &Capabilities) -> Self {
        Self {
            images_supported: capabilities.images,
            ..Self::default()
        }
    }
}
/// Read regular files within explicit byte limits. MIME detection uses signatures, never extensions.
/// UTF-8 text is inline; unknown binary files are rejected. Path references require an explicit policy.
pub async fn prepare_files<P: AsRef<Path>>(
    paths: &[P],
    options: &AttachmentOptions,
) -> Result<Vec<PromptContent>> {
    if options.max_image_bytes == 0 || options.max_text_bytes == 0 || options.max_total_bytes == 0 {
        return Err(Error::Invalid(
            "attachment byte limits must be positive".into(),
        ));
    }
    if options.max_image_bytes > 8 * 1024 * 1024 || options.max_text_bytes > 1024 * 1024 {
        return Err(Error::Invalid(
            "attachment limits exceed the maximum supported size".into(),
        ));
    }
    let root = match &options.root {
        Some(root) => Some(tokio::fs::canonicalize(root).await?),
        None => None,
    };
    let mut total = 0usize;
    let mut result = Vec::with_capacity(paths.len());
    for path in paths {
        let path = tokio::fs::canonicalize(path.as_ref()).await?;
        if root.as_ref().is_some_and(|root| !path.starts_with(root)) {
            return Err(Error::Invalid(
                "attachment is outside the allowed root".into(),
            ));
        }
        if !tokio::fs::metadata(&path).await?.is_file() {
            return Err(Error::Invalid("attachment must be a regular file".into()));
        }
        let file = tokio::fs::File::open(&path).await?;
        let limit = options
            .max_image_bytes
            .max(options.max_text_bytes)
            .min(options.max_total_bytes.saturating_sub(total));
        let read_limit = limit
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("attachment limit is too large".into()))?;
        let mut bytes = Vec::new();
        file.take(read_limit as u64).read_to_end(&mut bytes).await?;
        if bytes.len() > limit {
            return Err(Error::Invalid("attachment exceeds the byte limit".into()));
        }
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| Error::Invalid("attachment size overflow".into()))?;
        let content = if let Some(mime) = image_mime(&bytes) {
            if bytes.len() > options.max_image_bytes {
                return Err(Error::Invalid(
                    "image attachment exceeds the byte limit".into(),
                ));
            }
            if options.images_supported {
                PromptContent::Image {
                    data: encode_base64(&bytes),
                    mime_type: mime.into(),
                }
            } else if options.unsupported_image == UnsupportedImage::PathReference {
                PromptContent::Text(format!(
                    "Attached image file (path reference): {}",
                    path.display()
                ))
            } else {
                return Err(Error::Unsupported("inline image attachments".into()));
            }
        } else {
            if bytes.len() > options.max_text_bytes {
                return Err(Error::Invalid(
                    "text attachment exceeds the byte limit".into(),
                ));
            }
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::Invalid("unknown binary attachment".into()))?;
            if text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(Error::Invalid("unknown binary attachment".into()));
            }
            PromptContent::Text(format!("File: {}\n{text}", path.display()))
        };
        result.push(content);
    }
    Ok(result)
}
fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}
fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        encoded.push(ALPHABET[(value >> 18) as usize] as char);
        encoded.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn prepared_files_are_bounded_and_image_fallback_is_explicit() {
        let root = std::env::temp_dir().join(format!("mpai-attachments-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let text_path = root.join("notes.txt");
        let image_path = root.join("wrong-extension.txt");
        let binary_path = root.join("unknown.bin");
        tokio::fs::write(&text_path, "Hello file").await.unwrap();
        tokio::fs::write(&image_path, b"\x89PNG\r\n\x1a\n")
            .await
            .unwrap();
        tokio::fs::write(&binary_path, b"\x00\xff").await.unwrap();
        let mut options = AttachmentOptions {
            root: Some(root.clone()),
            ..AttachmentOptions::default()
        };
        let prepared = prepare_files(&[&text_path], &options).await.unwrap();
        assert!(matches!(&prepared[0],PromptContent::Text(text) if text.ends_with("Hello file")));
        assert!(matches!(
            prepare_files(&[&image_path], &options).await,
            Err(Error::Unsupported(_))
        ));
        options.unsupported_image = UnsupportedImage::PathReference;
        let prepared = prepare_files(&[&image_path], &options).await.unwrap();
        assert!(
            matches!(&prepared[0],PromptContent::Text(text) if text.contains("path reference") && text.contains("wrong-extension.txt"))
        );
        options.images_supported = true;
        let prepared = prepare_files(&[&image_path], &options).await.unwrap();
        assert!(
            matches!(&prepared[0],PromptContent::Image { data,mime_type } if data == "iVBORw0KGgo=" && mime_type == "image/png")
        );
        assert!(matches!(
            prepare_files(&[&binary_path], &options).await,
            Err(Error::Invalid(_))
        ));
        options.max_text_bytes = 3;
        assert!(matches!(
            prepare_files(&[&text_path], &options).await,
            Err(Error::Invalid(_))
        ));
        options.max_text_bytes = 1024;
        options.max_total_bytes = 12;
        assert!(matches!(
            prepare_files(&[&text_path, &image_path], &options).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            prepare_files(&[&root], &options).await,
            Err(Error::Invalid(_))
        ));
        let confined = root.join("confined");
        tokio::fs::create_dir_all(&confined).await.unwrap();
        options.root = Some(confined);
        assert!(matches!(
            prepare_files(&[&text_path], &options).await,
            Err(Error::Invalid(_))
        ));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[test]
    fn signature_detection_and_standard_base64_padding() {
        for (bytes, expected) in [
            (b"".as_slice(), ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode_base64(bytes), expected);
        }
        assert_eq!(image_mime(b"GIF89a"), Some("image/gif"));
        assert_eq!(image_mime(b"\xff\xd8\xff"), Some("image/jpeg"));
        assert_eq!(image_mime(b"RIFF1234WEBP"), Some("image/webp"));
        assert_eq!(image_mime(b"not actually an image.png"), None);
    }
}
