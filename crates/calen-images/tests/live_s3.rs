//! Uploads to a real S3-compatible server when `CALEN_TEST_S3=endpoint,bucket,key,secret[,region]`
//! is set (e.g. MinIO or moto on localhost). Skipped otherwise.

use calen_images::{Provider, S3Config};

#[tokio::test(flavor = "multi_thread")]
async fn upload_and_read_back() {
    let Ok(spec) = std::env::var("CALEN_TEST_S3") else {
        eprintln!("CALEN_TEST_S3 not set; skipping");
        return;
    };
    let p: Vec<&str> = spec.split(',').collect();
    let cfg = S3Config {
        endpoint: p[0].into(),
        bucket: p[1].into(),
        access_key_id: p[2].into(),
        secret_access_key: p[3].into(),
        region: p.get(4).copied().unwrap_or("us-east-1").into(),
        prefix: "calen-test/".into(),
        path_style: true,
        public_acl: true,
        ..Default::default()
    };
    let dir = std::env::temp_dir().join(format!("calen-s3-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("photo one.png");
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 200, 30, 255]));
    img.save(&file).unwrap();

    let urls = Provider::S3(cfg).upload(std::slice::from_ref(&file), "Test event").await.expect("upload");
    assert_eq!(urls.len(), 1);
    let url = &urls[0];
    assert!(url.contains("/calen-test/") && url.ends_with(".png"), "{url}");

    let back = reqwest::get(url).await.unwrap();
    assert!(back.status().is_success(), "GET {url}: {}", back.status());
    assert_eq!(back.bytes().await.unwrap().as_ref(), std::fs::read(&file).unwrap().as_slice());

    // Thumbnail pipeline works on the uploaded object.
    let thumb = calen_images::thumbnail(url, &dir).await.expect("thumbnail");
    assert_eq!((thumb.width, thumb.height), (8, 8));
    let _ = std::fs::remove_dir_all(&dir);
}
