//! User preferences, persisted as one JSON blob in the store.

use calen_core::quickadd::DateOrder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewKind {
    Day,
    #[default]
    Week,
    Month,
}

/// Where attached images are uploaded. Off by default (uploads are optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageHost {
    #[default]
    Off,
    Felix,
    ImgBB,
    S3,
}

impl ImageHost {
    pub const ALL: [ImageHost; 4] = [ImageHost::Off, ImageHost::Felix, ImageHost::ImgBB, ImageHost::S3];

    pub fn label(self) -> &'static str {
        match self {
            ImageHost::Off => "Off",
            ImageHost::Felix => "img.felixadr.com",
            ImageHost::ImgBB => "ImgBB",
            ImageHost::S3 => "S3-compatible storage",
        }
    }
}

impl std::fmt::Display for ImageHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Non-secret S3 settings (the secret key lives in the OS keyring).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct S3Settings {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub prefix: String,
    pub public_base_url: String,
    pub path_style: bool,
    pub public_acl: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    /// 0 = Monday … 6 = Sunday.
    pub week_start: u8,
    pub time_24h: bool,
    pub date_order: DateOrder,
    pub default_duration: i64,
    pub default_alert: Option<i64>,
    pub default_calendar: Option<String>,
    pub close_to_tray: bool,
    pub start_at_login: bool,
    pub notifications: bool,
    /// Unix seconds; DND active while now < this. i64::MAX = until turned off.
    pub dnd_until: Option<i64>,
    pub respect_system_dnd: bool,
    pub image_public: bool,
    pub image_host: ImageHost,
    pub s3: S3Settings,
    pub sidebar_visible: bool,
    pub view: ViewKind,
    pub first_run_done: bool,
    pub auto_update: bool,
    pub day_start_hour: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeChoice::System,
            week_start: 0,
            time_24h: true,
            date_order: DateOrder::DayMonth,
            default_duration: 60,
            default_alert: Some(10),
            default_calendar: None,
            close_to_tray: true,
            start_at_login: true,
            notifications: true,
            dnd_until: None,
            respect_system_dnd: true,
            image_public: true,
            image_host: ImageHost::Off,
            s3: S3Settings::default(),
            sidebar_visible: true,
            view: ViewKind::Week,
            first_run_done: false,
            auto_update: true,
            day_start_hour: 7,
        }
    }
}

impl Settings {
    pub const KEY: &'static str = "settings";

    pub fn week_start_day(&self) -> chrono::Weekday {
        use chrono::Weekday::*;
        [Mon, Tue, Wed, Thu, Fri, Sat, Sun][(self.week_start as usize).min(6)]
    }

    pub fn dnd_active(&self, now: i64) -> bool {
        self.dnd_until.is_some_and(|u| now < u)
    }
}
