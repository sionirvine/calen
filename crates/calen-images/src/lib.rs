//! Image hosting for event attachments + local thumbnail cache.
//!
//! Providers: img.felixadr.com, ImgBB, or any S3-compatible bucket ([`Provider`]).
//!
//! img.felixadr.com API notes:
//!
//! API (Bearer token):
//! - `POST /api/v1/posts` multipart `file` (repeatable), `title`, `tags`, `public` → post JSON
//! - `POST /api/v1/posts/{id}/images` multipart `file` → post/images JSON
//! - post JSON: `{ id, url, images: [{ id, url, thumb_url, width, height, ... }] }`
//!
//! Thumbnails served by the API are AVIF only, so we fetch the full image as
//! JPEG/PNG/WebP (content negotiation) and downscale locally.

pub mod imgbb;
pub mod s3;

pub use s3::S3Config;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const API_BASE: &str = "https://img.felixadr.com";
pub const THUMB_SIZE: u32 = 256;
const ACCEPT: &str = "image/jpeg,image/png,image/webp,image/gif;q=0.9,*/*;q=0.1";

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("no image API key set (Ctrl+P → \"> Image upload: set API key\")")]
    NoKey,
    #[error("network: {0}")]
    Net(String),
    #[error("upload rejected: {0}")]
    Api(String),
    #[error("not an image I can read: {0}")]
    Decode(String),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteImage {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub thumb_url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Post {
    pub id: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub images: Vec<RemoteImage>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PostResponse {
    Wrapped { post: Post },
    Plain(Post),
    Images { images: Vec<RemoteImage> },
}

pub const IMAGE_EXTENSIONS: [&str; 9] = ["jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "avif", "bmp"];

pub fn is_image_path(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

pub(crate) fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("Calen/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

pub(crate) fn mime_for(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

pub(crate) async fn file_part(path: &Path) -> Result<Part, ImageError> {
    let bytes = tokio::fs::read(path).await?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
    Part::bytes(bytes).file_name(name).mime_str(mime_for(path)).map_err(|e| ImageError::Api(e.to_string()))
}

async fn parse_response(resp: reqwest::Response) -> Result<Vec<RemoteImage>, ImageError> {
    let status = resp.status();
    let body = resp.text().await.map_err(|e| ImageError::Net(e.to_string()))?;
    if !status.is_success() {
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_else(|| body.chars().take(200).collect());
        return Err(ImageError::Api(format!("{status}: {msg}")));
    }
    let parsed: PostResponse = serde_json::from_str(&body).map_err(|e| ImageError::Api(format!("unexpected response: {e}")))?;
    Ok(match parsed {
        PostResponse::Wrapped { post } => post.images,
        PostResponse::Plain(post) => post.images,
        PostResponse::Images { images } => images,
    })
}

/// Where uploaded images go.
#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    /// img.felixadr.com (Bearer API key).
    Felix { key: String, public: bool },
    /// ImgBB (API key from https://api.imgbb.com).
    ImgBB { key: String },
    /// Your own S3-compatible bucket.
    S3(S3Config),
}

impl Provider {
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Felix { .. } => "img.felixadr.com",
            Provider::ImgBB { .. } => "ImgBB",
            Provider::S3(_) => "S3",
        }
    }

    /// Upload files; returns one public URL per file, in order.
    pub async fn upload(&self, files: &[PathBuf], title: &str) -> Result<Vec<String>, ImageError> {
        match self {
            Provider::Felix { key, public } => Ok(upload(key, files, title, *public).await?.1.into_iter().map(|i| i.url).collect()),
            Provider::ImgBB { key } => {
                let mut out = Vec::new();
                for f in files {
                    out.push(imgbb::upload(key, f, title).await?);
                }
                Ok(out)
            }
            Provider::S3(cfg) => {
                let mut out = Vec::new();
                for f in files {
                    out.push(s3::put_file(cfg, f).await?);
                }
                Ok(out)
            }
        }
    }
}

/// Upload files as one new post. Returns the stored images (in order).
pub async fn upload(key: &str, files: &[PathBuf], title: &str, public: bool) -> Result<(Option<String>, Vec<RemoteImage>), ImageError> {
    if key.is_empty() {
        return Err(ImageError::NoKey);
    }
    let mut form = Form::new().text("title", title.to_string()).text("tags", "calen").text("public", public.to_string());
    for f in files {
        form = form.part("file", file_part(f).await?);
    }
    let resp = http()
        .post(format!("{API_BASE}/api/v1/posts"))
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| ImageError::Net(e.to_string()))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| ImageError::Net(e.to_string()))?;
    if !status.is_success() {
        return Err(ImageError::Api(format!("{status}: {}", body.chars().take(200).collect::<String>())));
    }
    let post_id = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("id").or_else(|| v.get("post").and_then(|p| p.get("id"))).and_then(|i| i.as_str()).map(String::from));
    let parsed: PostResponse = serde_json::from_str(&body).map_err(|e| ImageError::Api(format!("unexpected response: {e}")))?;
    let images = match parsed {
        PostResponse::Wrapped { post } => post.images,
        PostResponse::Plain(post) => post.images,
        PostResponse::Images { images } => images,
    };
    Ok((post_id, images))
}

/// Append files to an existing post.
pub async fn add_to_post(key: &str, post_id: &str, files: &[PathBuf]) -> Result<Vec<RemoteImage>, ImageError> {
    let mut form = Form::new();
    for f in files {
        form = form.part("file", file_part(f).await?);
    }
    let resp = http()
        .post(format!("{API_BASE}/api/v1/posts/{post_id}/images"))
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| ImageError::Net(e.to_string()))?;
    parse_response(resp).await
}

/// Decoded RGBA pixels.
#[derive(Debug, Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Where [`thumbnail`] caches the thumbnail of `url` inside `cache_dir`.
pub fn thumbnail_path(url: &str, cache_dir: &Path) -> std::path::PathBuf {
    cache_dir.join(cache_name(url, "-t"))
}

fn cache_name(url: &str, suffix: &str) -> String {
    // FNV-1a — stable, no extra deps.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}{suffix}.png")
}

async fn download(url: &str) -> Result<Vec<u8>, ImageError> {
    let resp = http().get(url).header("Accept", ACCEPT).send().await.map_err(|e| ImageError::Net(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ImageError::Net(format!("{} for {url}", resp.status())));
    }
    Ok(resp.bytes().await.map_err(|e| ImageError::Net(e.to_string()))?.to_vec())
}

fn decode(bytes: &[u8]) -> Result<image::DynamicImage, ImageError> {
    image::load_from_memory(bytes).map_err(|e| ImageError::Decode(e.to_string()))
}

/// Downscale to fit the thumbnail box; never upscale small images.
fn shrink(img: image::DynamicImage) -> image::DynamicImage {
    if img.width() > THUMB_SIZE || img.height() > THUMB_SIZE { img.thumbnail(THUMB_SIZE, THUMB_SIZE) } else { img }
}

fn to_rgba(img: image::DynamicImage) -> Rgba {
    let rgba = img.into_rgba8();
    Rgba { width: rgba.width(), height: rgba.height(), pixels: rgba.into_raw() }
}

/// Thumbnail (≤256 px, cropped square-ish by the UI), cached on disk.
pub async fn thumbnail(url: &str, cache_dir: &Path) -> Result<Rgba, ImageError> {
    let path = thumbnail_path(url, cache_dir);
    if let Ok(bytes) = tokio::fs::read(&path).await
        && let Ok(img) = decode(&bytes)
    {
        return Ok(to_rgba(img));
    }
    let bytes = download(url).await?;
    let thumb = tokio::task::spawn_blocking(move || -> Result<image::DynamicImage, ImageError> { Ok(shrink(decode(&bytes)?)) })
        .await
        .map_err(|e| ImageError::Decode(e.to_string()))??;
    let _ = tokio::fs::create_dir_all(cache_dir).await;
    let mut png = Vec::new();
    if thumb.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
        let _ = tokio::fs::write(&path, &png).await;
    }
    Ok(to_rgba(thumb))
}

/// Full-size image for the viewer (downscaled to at most 4096 px).
pub async fn full(url: &str) -> Result<Rgba, ImageError> {
    let bytes = download(url).await?;
    tokio::task::spawn_blocking(move || {
        let img = decode(&bytes)?;
        let img = if img.width() > 4096 || img.height() > 4096 { img.thumbnail(4096, 4096) } else { img };
        Ok(to_rgba(img))
    })
    .await
    .map_err(|e| ImageError::Decode(e.to_string()))?
}

/// Local preview of a file being uploaded.
pub async fn local_thumbnail(path: PathBuf) -> Result<Rgba, ImageError> {
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path)?;
        Ok(to_rgba(shrink(decode(&bytes)?)))
    })
    .await
    .map_err(|e| ImageError::Decode(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_post_shapes() {
        let plain = r#"{"id":"p1","url":"https://img.felixadr.com/a/p1","images":[{"id":"i1","url":"https://img.felixadr.com/i/i1","thumb_url":"https://img.felixadr.com/t/i1","width":10,"height":20}]}"#;
        let PostResponse::Plain(p) = serde_json::from_str::<PostResponse>(plain).unwrap() else { panic!() };
        assert_eq!(p.images[0].url, "https://img.felixadr.com/i/i1");
        let wrapped = r#"{"post":{"id":"p1","images":[{"id":"i1","url":"u"}]}}"#;
        assert!(matches!(serde_json::from_str::<PostResponse>(wrapped).unwrap(), PostResponse::Wrapped { .. }));
        assert!(is_image_path(Path::new("C:/x/IMG_1.HEIC")));
        assert!(!is_image_path(Path::new("notes.txt")));
    }
}
