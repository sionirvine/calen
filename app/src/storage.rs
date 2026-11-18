//! Optional image hosting: choose Off / img.felixadr.com / ImgBB / your own
//! S3-compatible bucket. Secrets live in the OS keyring; the rest in settings.

use calen_images::{Provider, S3Config};
use iced::widget::{Space, button, checkbox, column, container, pick_list, row, text, text_input};
use iced::{Alignment, Element, Font, Length, Task, font};

use crate::app::{App, Message};
use crate::settings::{ImageHost, S3Settings};
use crate::theme::{self, Palette};

const KEYRING_SERVICE: &str = "calen-image-api";

fn secret_slot(host: ImageHost) -> &'static str {
    match host {
        ImageHost::Felix => "default",
        ImageHost::ImgBB => "imgbb",
        ImageHost::S3 => "s3",
        ImageHost::Off => "",
    }
}

pub fn secret(host: ImageHost) -> Option<String> {
    let slot = secret_slot(host);
    if slot.is_empty() {
        return None;
    }
    if let Ok(k) = keyring::Entry::new(KEYRING_SERVICE, slot).and_then(|e| e.get_password())
        && !k.is_empty()
    {
        return Some(k);
    }
    // Development convenience only — never compiled into release builds.
    if cfg!(debug_assertions) && host == ImageHost::Felix {
        if let Ok(k) = std::env::var("IMG_API_KEY") {
            return Some(k);
        }
        if let Ok(env) = std::fs::read_to_string(".env") {
            for line in env.lines() {
                if let Some(v) = line.trim().strip_prefix("IMG_API_KEY=") {
                    return Some(v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}

fn save_secret(host: ImageHost, value: &str) -> Result<(), String> {
    let e = keyring::Entry::new(KEYRING_SERVICE, secret_slot(host)).map_err(|e| e.to_string())?;
    e.set_password(value).map_err(|e| e.to_string())
}

fn s3_config(s: &S3Settings, secret_access_key: String) -> S3Config {
    S3Config {
        endpoint: s.endpoint.trim().to_string(),
        region: s.region.trim().to_string(),
        bucket: s.bucket.trim().to_string(),
        access_key_id: s.access_key_id.trim().to_string(),
        secret_access_key,
        prefix: s.prefix.trim().to_string(),
        public_base_url: s.public_base_url.trim().to_string(),
        path_style: s.path_style,
        public_acl: s.public_acl,
    }
}

/// The configured upload target, or a message explaining what's missing.
pub fn provider(app: &App) -> Result<Provider, String> {
    let host = app.settings.image_host;
    let missing = || format!("Image uploads need a {} key — Ctrl+P → > Image storage", host.label());
    match host {
        ImageHost::Off => Err("Image uploads are off — turn them on in Ctrl+P → > Image storage".into()),
        ImageHost::Felix => Ok(Provider::Felix { key: secret(host).ok_or_else(missing)?, public: app.settings.image_public }),
        ImageHost::ImgBB => Ok(Provider::ImgBB { key: secret(host).ok_or_else(missing)? }),
        ImageHost::S3 => Ok(Provider::S3(s3_config(&app.settings.s3, secret(host).ok_or_else(missing)?))),
    }
}

pub fn enabled(app: &App) -> bool {
    app.settings.image_host != ImageHost::Off
}

// ---------------------------------------------------------------------------
// Setup dialog

#[derive(Debug, Clone, Default)]
pub struct StorageDialog {
    pub host: ImageHost,
    pub key: String,
    pub has_saved_key: bool,
    pub s3: S3Settings,
    pub public: bool,
    pub busy: bool,
    pub status: Option<Result<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Endpoint,
    Region,
    Bucket,
    AccessKey,
    Prefix,
    PublicBase,
}

#[derive(Debug, Clone)]
pub enum StorageMsg {
    Host(ImageHost),
    Key(String),
    S3(Field, String),
    PathStyle(bool),
    PublicAcl(bool),
    Public(bool),
    Test,
    Tested(Result<String, String>),
    Save,
    Close,
}

pub fn open(app: &mut App) -> Task<Message> {
    let host = if app.settings.image_host == ImageHost::Off { ImageHost::S3 } else { app.settings.image_host };
    let mut d = StorageDialog { host, s3: app.settings.s3.clone(), public: app.settings.image_public, ..Default::default() };
    d.has_saved_key = secret(host).is_some();
    if d.s3.region.is_empty() {
        d.s3.region = "auto".into();
    }
    if d.s3.prefix.is_empty() {
        d.s3.prefix = "calen/".into();
    }
    app.palette_state = None;
    app.storage = Some(d);
    app.open_main()
}

/// A tiny valid PNG used for the S3 connection test.
fn test_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([0xe5, 0x48, 0x4d, 0xff]));
    let mut out = Vec::new();
    let _ = image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    out
}

pub fn update(app: &mut App, m: StorageMsg) -> Task<Message> {
    let Some(d) = &mut app.storage else { return Task::none() };
    match m {
        StorageMsg::Host(h) => {
            d.host = h;
            d.key.clear();
            d.has_saved_key = secret(h).is_some();
            d.status = None;
        }
        StorageMsg::Key(k) => d.key = k,
        StorageMsg::S3(f, v) => {
            let s = &mut d.s3;
            match f {
                Field::Endpoint => s.endpoint = v,
                Field::Region => s.region = v,
                Field::Bucket => s.bucket = v,
                Field::AccessKey => s.access_key_id = v,
                Field::Prefix => s.prefix = v,
                Field::PublicBase => s.public_base_url = v,
            }
            d.status = None;
        }
        StorageMsg::PathStyle(b) => d.s3.path_style = b,
        StorageMsg::PublicAcl(b) => d.s3.public_acl = b,
        StorageMsg::Public(b) => d.public = b,
        StorageMsg::Test => {
            let Some(secret_key) = (!d.key.trim().is_empty()).then(|| d.key.trim().to_string()).or_else(|| secret(ImageHost::S3)) else {
                d.status = Some(Err("Enter the secret access key".into()));
                return Task::none();
            };
            let cfg = s3_config(&d.s3, secret_key);
            d.busy = true;
            d.status = None;
            let mut prefix = cfg.prefix.trim().trim_start_matches('/').to_string();
            if !prefix.is_empty() && !prefix.ends_with('/') {
                prefix.push('/');
            }
            let key = format!("{prefix}calen-connection-test.png");
            return Task::perform(
                async move {
                    let url = calen_images::s3::put_bytes(&cfg, &key, test_png(), "image/png").await.map_err(|e| e.to_string())?;
                    // Other calendar apps open links without credentials, so check that too.
                    let readable = reqwest::get(&url).await.map(|r| r.status().is_success()).unwrap_or(false);
                    if readable {
                        Ok(format!("Upload works and the image is public: {url}"))
                    } else {
                        Err(format!(
                            "Uploaded, but {url} isn't publicly readable. Set a public URL (CDN / R2 public bucket) or allow public reads in the bucket policy."
                        ))
                    }
                },
                |r| Message::Storage(StorageMsg::Tested(r)),
            );
        }
        StorageMsg::Tested(r) => {
            d.busy = false;
            d.status = Some(r);
        }
        StorageMsg::Save => {
            let d = d.clone();
            let key = d.key.trim().to_string();
            if !key.is_empty() {
                if let Err(e) = save_secret(d.host, &key) {
                    app.storage.as_mut().unwrap().status = Some(Err(format!("Couldn't store the key: {e}")));
                    return Task::none();
                }
            } else if !d.has_saved_key {
                let what = if d.host == ImageHost::S3 { "secret access key" } else { "API key" };
                app.storage.as_mut().unwrap().status = Some(Err(format!("Enter the {what}")));
                return Task::none();
            }
            if d.host == ImageHost::S3
                && (d.s3.endpoint.trim().is_empty() || d.s3.bucket.trim().is_empty() || d.s3.access_key_id.trim().is_empty())
            {
                app.storage.as_mut().unwrap().status = Some(Err("Endpoint, bucket and access key ID are required".into()));
                return Task::none();
            }
            app.settings.image_host = d.host;
            app.settings.s3 = d.s3;
            app.settings.image_public = d.public;
            app.save_settings();
            app.storage = None;
            app.flash(format!("Images will upload to {}", app.settings.image_host.label()));
        }
        StorageMsg::Close => app.storage = None,
    }
    Task::none()
}

pub fn turn_off(app: &mut App) {
    app.settings.image_host = ImageHost::Off;
    app.save_settings();
    app.flash("Image uploads are off. Existing image links still show.");
}

pub fn view<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let d = app.storage.as_ref()?;
    let bold = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };
    let field = |label: &'static str, placeholder: &'static str, value: &str, f: Field| {
        column![
            text(label).size(12).color(p.muted),
            text_input(placeholder, value)
                .on_input(move |v| Message::Storage(StorageMsg::S3(f, v)))
                .on_submit(Message::Storage(StorageMsg::Save))
                .style(theme::input)
                .size(13)
                .padding([6, 9]),
        ]
        .spacing(3)
    };
    let key_input = |label: &'static str, placeholder: &'static str| {
        let ph = if d.has_saved_key { "Saved — leave empty to keep" } else { placeholder };
        column![
            text(label).size(12).color(p.muted),
            text_input(ph, &d.key)
                .on_input(|v| Message::Storage(StorageMsg::Key(v)))
                .on_submit(Message::Storage(StorageMsg::Save))
                .secure(true)
                .style(theme::input)
                .size(13)
                .padding([6, 9]),
        ]
        .spacing(3)
    };
    let pick_style = move |theme: &iced::Theme, status| {
        let mut s = pick_list::default(theme, status);
        s.background = p.bg.into();
        s.border = iced::Border { color: p.grid_strong, width: 1.0, radius: 8.0.into() };
        s.text_color = p.text;
        s
    };

    let mut body = column![
        text("Image storage").size(17).font(bold),
        text("Where images you attach to events are uploaded. Links are saved in the event notes so other calendar apps can open them.")
            .size(12)
            .color(p.muted),
        row![
            text("Upload to").size(13).width(Length::Fixed(80.0)),
            pick_list(ImageHost::ALL, Some(d.host), |h| Message::Storage(StorageMsg::Host(h)))
                .text_size(13)
                .padding([6, 9])
                .style(pick_style),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(12);

    match d.host {
        ImageHost::Off => {
            body = body.push(text("Uploads are disabled. Existing image links in events are still shown.").size(12).color(p.muted));
        }
        ImageHost::Felix => {
            body = body.push(key_input("API key", "imk_…"));
            body = body.push(
                checkbox(d.public)
                    .label("Make uploaded images public (needed for other apps to open the links)")
                    .on_toggle(|b| Message::Storage(StorageMsg::Public(b)))
                    .size(15)
                    .text_size(12),
            );
        }
        ImageHost::ImgBB => {
            body = body.push(key_input("ImgBB API key", "from api.imgbb.com"));
            body = body.push(text("Images on ImgBB are public to anyone with the link.").size(12).color(p.muted));
        }
        ImageHost::S3 => {
            let s = &d.s3;
            body = body
                .push(field(
                    "Endpoint",
                    "https://<account>.r2.cloudflarestorage.com · https://s3.eu-central-1.amazonaws.com",
                    &s.endpoint,
                    Field::Endpoint,
                ))
                .push(
                    row![
                        field("Bucket", "my-bucket", &s.bucket, Field::Bucket),
                        field("Region", "auto / us-east-1", &s.region, Field::Region)
                    ]
                    .spacing(10),
                )
                .push(field("Access key ID", "AKIA…", &s.access_key_id, Field::AccessKey))
                .push(key_input("Secret access key", "••••••••"))
                .push(row![field("Folder (key prefix)", "calen/", &s.prefix, Field::Prefix)].spacing(10))
                .push(field(
                    "Public URL (optional)",
                    "https://img.example.com or https://pub-….r2.dev",
                    &s.public_base_url,
                    Field::PublicBase,
                ))
                .push(
                    row![
                        checkbox(s.path_style)
                            .label("Path-style URLs (MinIO, some providers)")
                            .on_toggle(|b| Message::Storage(StorageMsg::PathStyle(b)))
                            .size(15)
                            .text_size(12),
                        checkbox(s.public_acl)
                            .label("Send public-read ACL")
                            .on_toggle(|b| Message::Storage(StorageMsg::PublicAcl(b)))
                            .size(15)
                            .text_size(12),
                    ]
                    .spacing(16),
                );
        }
    }

    if let Some(st) = &d.status {
        let (msg, color) = match st {
            Ok(m) => (m.clone(), p.success),
            Err(e) => (e.clone(), p.danger),
        };
        body = body.push(text(msg).size(12).color(color));
    }

    let mut footer = row![].spacing(8).align_y(Alignment::Center);
    if d.host == ImageHost::S3 {
        let b = button(text(if d.busy { "Testing…" } else { "Test upload" }).size(13)).style(theme::ghost).padding([6, 12]);
        footer = footer.push(if d.busy { b } else { b.on_press(Message::Storage(StorageMsg::Test)) });
    }
    footer = footer
        .push(Space::new().width(Length::Fill))
        .push(button(text("Cancel").size(13)).style(theme::ghost).padding([6, 12]).on_press(Message::Storage(StorageMsg::Close)))
        .push(button(text("Save").size(13)).style(theme::primary).padding([6, 14]).on_press(Message::Storage(StorageMsg::Save)));
    body = body.push(footer);

    Some(container(body).padding(20).width(Length::Fixed(540.0)).style(theme::popover).into())
}
