//! Upload to any S3-compatible bucket (AWS S3, Cloudflare R2, MinIO, Backblaze
//! B2, Wasabi, DigitalOcean Spaces…) with a hand-rolled AWS Signature V4 `PUT`.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::ImageError;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct S3Config {
    /// e.g. `https://s3.eu-central-1.amazonaws.com`, `https://<acct>.r2.cloudflarestorage.com`, `http://127.0.0.1:9000`
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Key prefix inside the bucket, e.g. `calen/`.
    pub prefix: String,
    /// Public URL that serves the bucket (CDN / custom domain / R2 public URL).
    /// When empty, the object URL on the endpoint is used.
    pub public_base_url: String,
    /// `https://endpoint/bucket/key` instead of `https://bucket.endpoint/key`.
    pub path_style: bool,
    /// Send `x-amz-acl: public-read` (AWS buckets with ACLs enabled; R2 rejects it).
    pub public_acl: bool,
}

type HmacSha256 = Hmac<Sha256>;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    m.update(data.as_bytes());
    m.finalize().into_bytes().to_vec()
}

/// RFC 3986 encoding as SigV4 wants it (keep `/` in paths).
fn uri_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Inputs to one signed request (separated so it can be tested against AWS's examples).
pub struct SignInput<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// Already URI-encoded path, starting with `/`.
    pub path: &'a str,
    pub query: &'a str,
    /// Lower-case header names → values, excluding `host`, must include `x-amz-content-sha256` and `x-amz-date`.
    pub headers: Vec<(String, String)>,
    pub payload_hash: &'a str,
    pub amz_date: &'a str,
    pub region: &'a str,
    pub access_key_id: &'a str,
    pub secret: &'a str,
}

/// Returns the `Authorization` header value.
pub fn sign(input: &SignInput) -> String {
    let mut headers: Vec<(String, String)> = input.headers.clone();
    headers.push(("host".into(), input.host.to_string()));
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{}\n", v.trim())).collect();
    let signed_headers = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let canonical_request =
        format!("{}\n{}\n{}\n{}\n{}\n{}", input.method, input.path, input.query, canonical_headers, signed_headers, input.payload_hash);
    let date = &input.amz_date[..8];
    let scope = format!("{date}/{}/s3/aws4_request", input.region);
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{}\n{scope}\n{}", input.amz_date, sha256_hex(canonical_request.as_bytes()));
    let k_date = hmac(format!("AWS4{}", input.secret).as_bytes(), date);
    let k_region = hmac(&k_date, input.region);
    let k_service = hmac(&k_region, "s3");
    let k_signing = hmac(&k_service, "aws4_request");
    let signature = hex(&hmac(&k_signing, &string_to_sign));
    format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}", input.access_key_id)
}

fn object_key(cfg: &S3Config, file: &Path) -> String {
    let ext = file.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_else(|| "jpg".into());
    let now = chrono::Utc::now();
    let mut prefix = cfg.prefix.trim().trim_start_matches('/').to_string();
    if !prefix.is_empty() && !prefix.ends_with('/') {
        prefix.push('/');
    }
    format!("{prefix}{}/{}.{ext}", now.format("%Y/%m"), uuid::Uuid::new_v4().simple())
}

/// Where to send the PUT: (url, host header, canonical path).
fn target(cfg: &S3Config, key: &str) -> Result<(String, String, String), ImageError> {
    let endpoint = cfg.endpoint.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(endpoint).map_err(|e| ImageError::Api(format!("invalid S3 endpoint: {e}")))?;
    let scheme = url.scheme();
    let host = url.host_str().ok_or_else(|| ImageError::Api("S3 endpoint has no host".into()))?;
    let host = match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    };
    let key_enc = uri_encode(key, true);
    if cfg.path_style {
        let path = format!("/{}/{key_enc}", uri_encode(&cfg.bucket, false));
        Ok((format!("{scheme}://{host}{path}"), host, path))
    } else {
        let vhost = format!("{}.{host}", cfg.bucket);
        let path = format!("/{key_enc}");
        Ok((format!("{scheme}://{vhost}{path}"), vhost, path))
    }
}

/// Upload one file; returns its public URL.
pub async fn put_file(cfg: &S3Config, file: &Path) -> Result<String, ImageError> {
    let body = tokio::fs::read(file).await?;
    let key = object_key(cfg, file);
    put_bytes(cfg, &key, body, crate::mime_for(file)).await
}

pub async fn put_bytes(cfg: &S3Config, key: &str, body: Vec<u8>, content_type: &str) -> Result<String, ImageError> {
    for (name, v) in
        [("endpoint", &cfg.endpoint), ("bucket", &cfg.bucket), ("access key", &cfg.access_key_id), ("secret key", &cfg.secret_access_key)]
    {
        if v.trim().is_empty() {
            return Err(ImageError::Api(format!("S3 {name} is not set")));
        }
    }
    let region = if cfg.region.trim().is_empty() { "us-east-1" } else { cfg.region.trim() };
    let (url, host, path) = target(cfg, key)?;
    let payload_hash = sha256_hex(&body);
    let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut headers = vec![
        ("content-type".to_string(), content_type.to_string()),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
        ("x-amz-date".to_string(), amz_date.clone()),
    ];
    if cfg.public_acl {
        headers.push(("x-amz-acl".into(), "public-read".into()));
    }
    let auth = sign(&SignInput {
        method: "PUT",
        host: &host,
        path: &path,
        query: "",
        headers: headers.clone(),
        payload_hash: &payload_hash,
        amz_date: &amz_date,
        region,
        access_key_id: cfg.access_key_id.trim(),
        secret: cfg.secret_access_key.trim(),
    });
    let mut req = crate::http().put(&url).header("Authorization", auth).body(body);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| ImageError::Net(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let code = text.split("<Code>").nth(1).and_then(|s| s.split('<').next()).unwrap_or("");
        let msg = text.split("<Message>").nth(1).and_then(|s| s.split('<').next()).unwrap_or("");
        return Err(ImageError::Api(format!("S3 {status} {code} {msg}").trim().to_string()));
    }
    Ok(public_url(cfg, key, &url))
}

fn public_url(cfg: &S3Config, key: &str, object_url: &str) -> String {
    let base = cfg.public_base_url.trim().trim_end_matches('/');
    if base.is_empty() { object_url.to_string() } else { format!("{base}/{}", uri_encode(key, true)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "GET Object" example from the AWS Signature Version 4 documentation for S3.
    #[test]
    fn aws_documented_example() {
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let auth = sign(&SignInput {
            method: "GET",
            host: "examplebucket.s3.amazonaws.com",
            path: "/test.txt",
            query: "",
            headers: vec![
                ("range".into(), "bytes=0-9".into()),
                ("x-amz-content-sha256".into(), empty.into()),
                ("x-amz-date".into(), "20130524T000000Z".into()),
            ],
            payload_hash: empty,
            amz_date: "20130524T000000Z",
            region: "us-east-1",
            access_key_id: "AKIAIOSFODNN7EXAMPLE",
            secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        });
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    /// PUT with ACL + content type, cross-checked against botocore's S3SigV4Auth.
    #[test]
    fn put_matches_botocore() {
        let hash = sha256_hex(b"hello calen");
        assert_eq!(hash, "ec7c8325b5185b49abf374b4bb272e7f2ce6523f6a223e0b165ca56ce312f8b9");
        let auth = sign(&SignInput {
            method: "PUT",
            host: "pics.s3.eu-central-1.amazonaws.com",
            path: "/calen/2026/11/a%20b.jpg",
            query: "",
            headers: vec![
                ("content-type".into(), "image/jpeg".into()),
                ("x-amz-acl".into(), "public-read".into()),
                ("x-amz-content-sha256".into(), hash.clone()),
                ("x-amz-date".into(), "20261118T093000Z".into()),
            ],
            payload_hash: &hash,
            amz_date: "20261118T093000Z",
            region: "eu-central-1",
            access_key_id: "AKIDEXAMPLE",
            secret: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        });
        assert!(auth.ends_with("Signature=c793fb16a66dc3609a79170009432ed4366c67ef4d67c337c75cfd94d6265cd4"), "{auth}");
    }

    #[test]
    fn targets_and_urls() {
        let mut cfg = S3Config { endpoint: "https://s3.eu-central-1.amazonaws.com/".into(), bucket: "pics".into(), ..Default::default() };
        let (url, host, path) = target(&cfg, "calen/2026/11/a b.jpg").unwrap();
        assert_eq!(host, "pics.s3.eu-central-1.amazonaws.com");
        assert_eq!(path, "/calen/2026/11/a%20b.jpg");
        assert_eq!(url, "https://pics.s3.eu-central-1.amazonaws.com/calen/2026/11/a%20b.jpg");
        cfg.path_style = true;
        cfg.endpoint = "http://127.0.0.1:9000".into();
        let (url, host, _) = target(&cfg, "x.png").unwrap();
        assert_eq!((url.as_str(), host.as_str()), ("http://127.0.0.1:9000/pics/x.png", "127.0.0.1:9000"));
        cfg.public_base_url = "https://img.example.com/".into();
        assert_eq!(public_url(&cfg, "calen/x.png", &url), "https://img.example.com/calen/x.png");
        cfg.prefix = "calen".into();
        assert!(object_key(&cfg, Path::new("C:/p/IMG.JPG")).starts_with("calen/"));
        assert!(object_key(&cfg, Path::new("C:/p/IMG.JPG")).ends_with(".jpg"));
    }
}
