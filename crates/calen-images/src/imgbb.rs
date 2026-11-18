//! ImgBB (https://api.imgbb.com): `POST /1/upload?key=…` with multipart `image`.
//! One image per request; the response's `data.url` is the direct image link.

use reqwest::multipart::Form;
use serde::Deserialize;
use std::path::Path;

use crate::ImageError;

#[derive(Deserialize)]
struct Resp {
    data: Option<Data>,
    error: Option<ErrorBody>,
}

#[derive(Deserialize)]
struct Data {
    url: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: Option<String>,
}

pub async fn upload(key: &str, file: &Path, name: &str) -> Result<String, ImageError> {
    if key.trim().is_empty() {
        return Err(ImageError::NoKey);
    }
    let mut form = Form::new().part("image", crate::file_part(file).await?);
    let stem = name.trim();
    if !stem.is_empty() {
        form = form.text("name", stem.chars().take(80).collect::<String>());
    }
    let mut url = reqwest::Url::parse("https://api.imgbb.com/1/upload").expect("static url");
    url.query_pairs_mut().append_pair("key", key.trim());
    let resp = crate::http().post(url).multipart(form).send().await.map_err(|e| ImageError::Net(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| ImageError::Net(e.to_string()))?;
    parse(status.as_u16(), &text)
}

fn parse(status: u16, text: &str) -> Result<String, ImageError> {
    let parsed: Resp = serde_json::from_str(text)
        .map_err(|_| ImageError::Api(format!("ImgBB {status}: {}", text.chars().take(160).collect::<String>())))?;
    match (parsed.data, parsed.error) {
        (Some(d), _) if (200..300).contains(&status) => Ok(d.url),
        (_, Some(e)) => Err(ImageError::Api(format!("ImgBB: {}", e.message.unwrap_or_else(|| status.to_string())))),
        _ => Err(ImageError::Api(format!("ImgBB returned {status}"))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_success_and_error() {
        let ok = r#"{"data":{"id":"2ndCYJK","url":"https://i.ibb.co/w04Prt6/c1f64245afb2.gif","display_url":"https://i.ibb.co/98W13PY/c1f64245afb2.gif"},"success":true,"status":200}"#;
        assert_eq!(super::parse(200, ok).unwrap(), "https://i.ibb.co/w04Prt6/c1f64245afb2.gif");
        let err = r#"{"status_code":400,"error":{"message":"Invalid API v1 key.","code":100},"status_txt":"Bad Request"}"#;
        assert!(super::parse(400, err).unwrap_err().to_string().contains("Invalid API v1 key"));
    }
}
