//! GNOME Online Accounts: offer the CalDAV accounts already set up in
//! Settings → Online Accounts (Nextcloud, WebDAV) so they needn't be typed in
//! again. Read once when the "Connect calendar" dialog opens; the password is
//! then copied into Calen's keyring like any other account.

use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

use super::GoaAccount;

const SERVICE: &str = "org.gnome.OnlineAccounts";
const ROOT: &str = "/org/gnome/OnlineAccounts";
const ACCOUNT: &str = "org.gnome.OnlineAccounts.Account";
const CALENDAR: &str = "org.gnome.OnlineAccounts.Calendar";
const PASSWORD: &str = "org.gnome.OnlineAccounts.PasswordBased";

fn string(props: &HashMap<String, OwnedValue>, key: &str) -> String {
    props.get(key).and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default()
}

fn flag(props: &HashMap<String, OwnedValue>, key: &str) -> bool {
    props.get(key).and_then(|v| bool::try_from(v).ok()).unwrap_or(false)
}

/// Accounts with calendars switched on, most usable first. Empty when GOA
/// isn't there (other desktops, or no permission in the sandbox).
pub fn accounts() -> Vec<GoaAccount> {
    let result = (|| -> zbus::Result<Vec<GoaAccount>> {
        let conn = zbus::blocking::Connection::session()?;
        let om = zbus::blocking::fdo::ObjectManagerProxy::builder(&conn).destination(SERVICE)?.path(ROOT)?.build()?;
        let mut out = Vec::new();
        for (path, ifaces) in om.get_managed_objects()? {
            let iface = |name: &str| ifaces.iter().find(|(n, _)| n.as_str() == name).map(|(_, p)| p);
            let (Some(account), Some(calendar)) = (iface(ACCOUNT), iface(CALENDAR)) else { continue };
            let uri = string(calendar, "Uri");
            if flag(account, "CalendarDisabled") || flag(account, "IsTemporary") || uri.is_empty() {
                continue;
            }
            out.push(GoaAccount {
                path: path.to_string(),
                provider: string(account, "ProviderName"),
                label: string(account, "PresentationIdentity"),
                uri,
                username: string(account, "Identity"),
                supported: iface(PASSWORD).is_some(),
            });
        }
        out.sort_by(|a, b| b.supported.cmp(&a.supported).then(a.provider.cmp(&b.provider)).then(a.label.cmp(&b.label)));
        Ok(out)
    })();
    result.unwrap_or_else(|e| {
        log::info!("GNOME Online Accounts unavailable: {e}");
        Vec::new()
    })
}

/// The account's password, from GOA (which keeps it in the keyring).
pub fn password(path: &str) -> Result<String, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let proxy = zbus::blocking::Proxy::new(&conn, SERVICE, path, PASSWORD).map_err(|e| e.to_string())?;
    proxy.call::<_, _, String>("GetPassword", &("",)).map_err(|e| match e {
        zbus::Error::MethodError(_, Some(msg), _) => msg,
        e => e.to_string(),
    })
}
