//! Self-update from GitHub Releases.
//!
//! 1. Check `releases/latest` of `CALEN_GITHUB_REPO` (baked in at build time by CI).
//! 2. Download this platform's asset in the background and verify it
//!    (minisign signature when `CALEN_UPDATE_PUBKEY` is baked in, SHA-256 otherwise).
//! 3. Stage it and show "Update ready — restart to apply".
//! 4. On the next launch (or "Restart now"), swap the executable / AppImage.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Subscription, Task};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::app::{App, Message};
use crate::icons;
use crate::theme::{self, Palette};

const REPO_ENV: Option<&str> = option_env!("CALEN_GITHUB_REPO");
const PUBKEY_ENV: Option<&str> = option_env!("CALEN_UPDATE_PUBKEY");

fn repo() -> Option<&'static str> {
    REPO_ENV.filter(|r| !r.trim().is_empty())
}

fn pubkey() -> Option<&'static str> {
    PUBKEY_ENV.map(str::trim).filter(|k| !k.is_empty())
}
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(windows)]
pub const ASSET: &str = "calen-windows-x86_64.exe";
#[cfg(target_os = "linux")]
pub const ASSET: &str = "Calen-x86_64.AppImage";
#[cfg(not(any(windows, target_os = "linux")))]
pub const ASSET: &str = "";

#[derive(Debug, Clone, Default, PartialEq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Ready(String),
    /// Newer version exists but this install can't self-update (dev build, non-AppImage).
    Available(String, String),
    UpToDate,
    Failed(String),
}

#[derive(Default)]
pub struct UpdaterState {
    pub status: UpdateStatus,
}

#[derive(Debug, Clone)]
pub enum UpdateMsg {
    Check {
        manual: bool,
    },
    Result {
        manual: bool,
        result: Result<UpdateStatus, String>,
    },
    RestartNow,
    OpenReleasePage(String),
    /// Hide the floating "update ready" notice for this version.
    DismissBanner,
}

#[derive(Serialize, Deserialize)]
struct Staged {
    version: String,
    path: PathBuf,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn update_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("update")
}

/// Numeric dotted version compare (`v1.2.10` > `1.2.9`).
pub fn newer(remote: &str, local: &str) -> bool {
    let parse =
        |s: &str| -> Vec<u64> { s.trim_start_matches('v').split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect() };
    parse(remote) > parse(local)
}

/// Can this install replace itself?
fn self_updatable() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    if cfg!(target_os = "linux") {
        return std::env::var_os("APPIMAGE").map(PathBuf::from);
    }
    std::env::current_exe().ok()
}

pub fn boot(app: &mut App) -> Task<Message> {
    // Dev aid: CALEN_FAKE_UPDATE=9.9.9 pretends an update is staged (debug builds only).
    if cfg!(debug_assertions)
        && let Ok(v) = std::env::var("CALEN_FAKE_UPDATE")
    {
        app.updater.status = UpdateStatus::Ready(v);
        return Task::none();
    }
    if !app.settings.auto_update || repo().is_none() {
        return Task::none();
    }
    Task::perform(tokio::time::sleep(Duration::from_secs(15)), |_| Message::Update(UpdateMsg::Check { manual: false }))
}

pub fn subscription(app: &App) -> Subscription<Message> {
    if app.settings.auto_update && repo().is_some() {
        iced::time::every(Duration::from_secs(6 * 3600)).map(|_| Message::Update(UpdateMsg::Check { manual: false }))
    } else {
        Subscription::none()
    }
}

async fn check(data_dir: PathBuf) -> Result<UpdateStatus, String> {
    let repo = repo().ok_or("this build has no update channel")?;
    let http = reqwest::Client::builder()
        .user_agent(concat!("Calen/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let rel: Release = http
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let version = rel.tag_name.trim_start_matches('v').to_string();
    if !newer(&version, VERSION) {
        return Ok(UpdateStatus::UpToDate);
    }
    let dir = update_dir(&data_dir);
    if let Ok(s) = std::fs::read_to_string(dir.join("staged.json"))
        && let Ok(st) = serde_json::from_str::<Staged>(&s)
        && st.version == version
        && st.path.exists()
    {
        return Ok(UpdateStatus::Ready(version));
    }
    if self_updatable().is_none() {
        return Ok(UpdateStatus::Available(version, rel.html_url));
    }
    let asset = rel.assets.iter().find(|a| a.name == ASSET).ok_or(format!("release {version} has no {ASSET}"))?;
    let bytes = http
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    // Verify.
    match pubkey() {
        Some(pk) => {
            let sig_asset = rel.assets.iter().find(|a| a.name == format!("{ASSET}.minisig")).ok_or("release is missing its signature")?;
            let sig = http
                .get(&sig_asset.browser_download_url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            let pk = minisign_verify::PublicKey::from_base64(pk).map_err(|e| e.to_string())?;
            let sig = minisign_verify::Signature::decode(&sig).map_err(|e| e.to_string())?;
            pk.verify(&bytes, &sig, false).map_err(|_| "signature check failed — update rejected".to_string())?;
        }
        None => {
            use sha2::Digest;
            let sum_asset = rel.assets.iter().find(|a| a.name == format!("{ASSET}.sha256")).ok_or("release is missing its checksum")?;
            let expected = http
                .get(&sum_asset.browser_download_url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            let expected = expected.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            let actual: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
            if expected != actual {
                return Err("checksum mismatch — update rejected".into());
            }
        }
    }

    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{version}-{ASSET}"));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    let staged = Staged { version: version.clone(), path };
    std::fs::write(dir.join("staged.json"), serde_json::to_string(&staged).unwrap()).map_err(|e| e.to_string())?;
    Ok(UpdateStatus::Ready(version))
}

pub fn update(app: &mut App, m: UpdateMsg) -> Task<Message> {
    match m {
        UpdateMsg::Check { manual } => {
            if repo().is_none() {
                if manual {
                    app.flash("Updates are only available in release builds from GitHub");
                }
                return Task::none();
            }
            if matches!(app.updater.status, UpdateStatus::Checking | UpdateStatus::Ready(_)) {
                if manual && let UpdateStatus::Ready(v) = &app.updater.status {
                    app.flash(format!("Version {v} is ready — restart to apply"));
                }
                return Task::none();
            }
            app.updater.status = UpdateStatus::Checking;
            let dir = app.data_dir.clone();
            return Task::perform(check(dir), move |result| Message::Update(UpdateMsg::Result { manual, result }));
        }
        UpdateMsg::Result { manual, result } => match result {
            Ok(s) => {
                if manual {
                    match &s {
                        UpdateStatus::UpToDate => app.flash(format!("Calen {VERSION} is up to date")),
                        UpdateStatus::Ready(v) => app.flash(format!("Update {v} is ready — restart to apply")),
                        UpdateStatus::Available(v, _) => app.flash(format!("Version {v} is available")),
                        _ => {}
                    }
                }
                app.updater.status = s;
            }
            Err(e) => {
                log::warn!("update check: {e}");
                if manual {
                    app.flash(format!("Update check failed: {e}"));
                }
                app.updater.status = UpdateStatus::Failed(e);
            }
        },
        UpdateMsg::RestartNow => {
            log::info!("restarting to apply the update");
            let _ = std::fs::remove_file(app.data_dir.join("instance.port"));
            if apply_staged(&app.data_dir) {
                return iced::exit();
            }
            app.flash("Couldn't apply the update");
        }
        UpdateMsg::OpenReleasePage(url) => crate::app::open_url(&url),
        UpdateMsg::DismissBanner => {
            if let UpdateStatus::Ready(v) = &app.updater.status {
                app.settings.dismissed_update = Some(v.clone());
                app.save_settings();
            }
        }
    }
    Task::none()
}

/// Apply a staged update and relaunch. Returns true if this process should exit.
pub fn apply_staged(data_dir: &Path) -> bool {
    let dir = update_dir(data_dir);
    let Ok(s) = std::fs::read_to_string(dir.join("staged.json")) else { return false };
    let _ = std::fs::remove_file(dir.join("staged.json"));
    let Ok(st) = serde_json::from_str::<Staged>(&s) else { return false };
    if !newer(&st.version, VERSION) || !st.path.exists() {
        let _ = std::fs::remove_file(&st.path);
        return false;
    }
    let Some(target) = self_updatable() else { return false };
    let result = if cfg!(windows) {
        self_replace::self_replace(&st.path).map_err(|e| e.to_string())
    } else {
        // AppImage: atomic rename over the original file.
        let tmp = target.with_extension("new");
        std::fs::copy(&st.path, &tmp)
            .and_then(|_| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
                }
                std::fs::rename(&tmp, &target)
            })
            .map_err(|e| e.to_string())
    };
    let _ = std::fs::remove_file(&st.path);
    match result {
        Ok(()) => {
            log::info!("updated to {}", st.version);
            let args: Vec<String> = std::env::args().skip(1).collect();
            match std::process::Command::new(&target).args(args).spawn() {
                Ok(_) => true,
                Err(e) => {
                    log::error!("relaunch failed: {e}");
                    false
                }
            }
        }
        Err(e) => {
            log::error!("applying update failed: {e}");
            false
        }
    }
}

/// Floating "Calen X is ready · restart" notice with a close button, shown until
/// the user restarts or dismisses it for that version.
pub fn banner<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let UpdateStatus::Ready(v) = &app.updater.status else { return None };
    if app.settings.dismissed_update.as_ref() == Some(v) {
        return None;
    }
    let fg = Color::WHITE;
    let muted = theme::with_alpha(fg, 0.7);
    // iced text has no underline; draw a 1 px rule under the label instead.
    let restart = button(
        column![
            text("Restart").size(12.5).color(p.accent),
            container(Space::new().width(Length::Fill).height(1))
                .style(move |_| container::Style { background: Some(p.accent.into()), ..Default::default() }),
        ]
        .width(Length::Shrink),
    )
    .style(theme::link)
    .padding([2, 2])
    .on_press(Message::Update(UpdateMsg::RestartNow));
    let close =
        button(text("×").size(15).color(muted)).style(theme::link).padding([0, 4]).on_press(Message::Update(UpdateMsg::DismissBanner));
    let content = row![
        icons::icon(icons::download(), 14.0, p.success),
        text(format!("Calen {v} is ready")).size(12.5).color(fg),
        text("·").size(12.5).color(muted),
        restart,
        text("to update").size(12.5).color(muted),
        close,
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    Some(container(content).padding([6, 12]).style(theme::pill(theme::with_alpha(Color::from_rgb8(0x22, 0x22, 0x26), 0.96), fg)).into())
}

pub fn badge<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    match &app.updater.status {
        UpdateStatus::Ready(v) => button(
            row![icons::icon(icons::download(), 13.0, p.success), text(format!("Update {v} ready")).size(11.5).color(p.success)]
                .spacing(5)
                .align_y(Alignment::Center),
        )
        .style(theme::side_ghost)
        .padding([4, 6])
        .on_press(Message::Update(UpdateMsg::RestartNow))
        .into(),
        UpdateStatus::Available(v, url) => button(text(format!("{v} available")).size(11.5).color(p.sidebar_muted))
            .style(theme::side_ghost)
            .padding([4, 6])
            .on_press(Message::Update(UpdateMsg::OpenReleasePage(url.clone())))
            .into(),
        _ => Space::new().into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions() {
        assert!(super::newer("v0.2.0", "0.1.9"));
        assert!(super::newer("1.10.0", "1.9.3"));
        assert!(!super::newer("0.1.0", "0.1.0"));
        assert!(!super::newer("v0.0.9", "0.1.0"));
    }
}
