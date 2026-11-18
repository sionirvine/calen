//! Minimal CalDAV client (RFC 4791 + RFC 6578 sync-collection).

use reqwest::{Method, StatusCode, Url, header};
use std::time::Duration;

use crate::xml::{self, Node};

#[derive(Debug, thiserror::Error)]
pub enum DavError {
    #[error("can't reach the server ({0})")]
    Offline(String),
    #[error("the server rejected the username or password")]
    Auth,
    #[error("the event changed on the server")]
    Conflict,
    #[error("not found")]
    NotFound,
    #[error("sync token expired")]
    TokenExpired,
    #[error("server error: {0}")]
    Http(String),
    #[error("couldn't understand the server response: {0}")]
    Parse(String),
}

pub type DavResult<T> = Result<T, DavError>;

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteCalendar {
    pub href: String,
    pub name: String,
    pub color: Option<String>,
    pub ctag: Option<String>,
    pub sync_token: Option<String>,
    pub read_only: bool,
}

#[derive(Debug, Default)]
pub struct Changes {
    pub changed: Vec<(String, Option<String>)>,
    pub deleted: Vec<String>,
    pub token: Option<String>,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: Url,
    user: String,
    pass: String,
}

fn map_err(e: reqwest::Error) -> DavError {
    if e.is_connect() || e.is_timeout() || e.is_request() { DavError::Offline(e.to_string()) } else { DavError::Http(e.to_string()) }
}

impl Client {
    pub fn new(base: &str, user: &str, pass: &str) -> DavResult<Self> {
        let mut s = base.trim().to_string();
        if !s.starts_with("http://") && !s.starts_with("https://") {
            s = format!("https://{s}");
        }
        if !s.ends_with('/') {
            s.push('/');
        }
        let base = Url::parse(&s).map_err(|e| DavError::Http(format!("invalid server URL: {e}")))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("Calen/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| DavError::Http(e.to_string()))?;
        Ok(Client { http, base, user: user.into(), pass: pass.into() })
    }

    pub fn url(&self, href: &str) -> DavResult<Url> {
        self.base.join(href).map_err(|e| DavError::Http(e.to_string()))
    }

    async fn send(
        &self,
        method: &str,
        url: Url,
        depth: Option<&str>,
        body: Option<String>,
        headers: &[(&str, &str)],
    ) -> DavResult<reqwest::Response> {
        let m = Method::from_bytes(method.as_bytes()).unwrap();
        let mut req = self.http.request(m, url).basic_auth(&self.user, Some(&self.pass));
        if let Some(d) = depth {
            req = req.header("Depth", d);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(b) = body {
            if method == "PUT" {
                req = req.header(header::CONTENT_TYPE, "text/calendar; charset=utf-8");
            } else {
                req = req.header(header::CONTENT_TYPE, "application/xml; charset=utf-8");
            }
            req = req.body(b);
        }
        let resp = req.send().await.map_err(map_err)?;
        match resp.status() {
            StatusCode::UNAUTHORIZED => Err(DavError::Auth),
            StatusCode::PRECONDITION_FAILED => Err(DavError::Conflict),
            StatusCode::NOT_FOUND | StatusCode::GONE => Err(DavError::NotFound),
            _ => Ok(resp),
        }
    }

    async fn xml_request(&self, method: &str, url: Url, depth: &str, body: String) -> DavResult<Node> {
        let resp = self.send(method, url, Some(depth), Some(body), &[]).await?;
        let status = resp.status();
        let text = resp.text().await.map_err(map_err)?;
        if status == StatusCode::FORBIDDEN && text.contains("valid-sync-token") {
            return Err(DavError::TokenExpired);
        }
        if !status.is_success() {
            if text.contains("valid-sync-token") {
                return Err(DavError::TokenExpired);
            }
            return Err(DavError::Http(format!("{status}")));
        }
        xml::parse(&text).map_err(DavError::Parse)
    }

    async fn propfind(&self, url: Url, depth: &str, props: &str) -> DavResult<Vec<Node>> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:ic="http://apple.com/ns/ical/"><d:prop>{props}</d:prop></d:propfind>"#
        );
        let root = self.xml_request("PROPFIND", url, depth, body).await?;
        let mut out = Vec::new();
        root.find_all("response", &mut out);
        Ok(out.into_iter().cloned().collect())
    }

    /// Follow principal → calendar-home-set → calendars.
    pub async fn discover(&self) -> DavResult<Vec<RemoteCalendar>> {
        let candidates = [self.base.as_str().to_string(), "/.well-known/caldav".into(), "/remote.php/dav/".into()];
        let mut principal = None;
        let mut last_err = None;
        for c in candidates {
            let url = self.url(&c)?;
            match self.propfind(url, "0", "<d:current-user-principal/>").await {
                Ok(resps) => {
                    if let Some(h) = resps.iter().find_map(|r| r.find("current-user-principal").and_then(|n| n.text_of("href"))) {
                        principal = Some(h);
                        break;
                    }
                }
                Err(e @ (DavError::Auth | DavError::Offline(_))) => return Err(e),
                Err(e) => last_err = Some(e),
            }
        }
        let principal = principal.ok_or_else(|| last_err.unwrap_or(DavError::Http("no CalDAV principal found at that address".into())))?;

        let resps = self.propfind(self.url(&principal)?, "0", "<c:calendar-home-set/>").await?;
        let home = resps
            .iter()
            .find_map(|r| r.find("calendar-home-set").and_then(|n| n.text_of("href")))
            .ok_or_else(|| DavError::Parse("no calendar-home-set".into()))?;

        let props = "<d:resourcetype/><d:displayname/><ic:calendar-color/><cs:getctag/><d:sync-token/><c:supported-calendar-component-set/><d:current-user-privilege-set/>";
        let resps = self.propfind(self.url(&home)?, "1", props).await?;
        let mut out = Vec::new();
        for r in resps {
            let Some(href) = r.text_of("href") else { continue };
            // Only the OK propstat holds values.
            let ok = r
                .children_named("propstat")
                .find(|ps| ps.text_of("status").is_some_and(|s| s.contains(" 200")))
                .cloned()
                .unwrap_or_default();
            let Some(rt) = ok.find("resourcetype") else { continue };
            if rt.child("calendar").is_none() {
                continue;
            }
            if let Some(set) = ok.find("supported-calendar-component-set") {
                let mut comps = Vec::new();
                set.find_all("comp", &mut comps);
                if !comps.is_empty() && !comps.iter().any(|c| c.attr("name").is_some_and(|n| n.eq_ignore_ascii_case("VEVENT"))) {
                    continue;
                }
            }
            if rt.child("subscribed").is_some() {
                continue;
            }
            let privs = ok.find("current-user-privilege-set");
            let read_only =
                privs.is_some_and(|p| p.find("write").is_none() && p.find("write-content").is_none() && p.find("all").is_none());
            out.push(RemoteCalendar {
                name: ok
                    .text_of("displayname")
                    .unwrap_or_else(|| href.trim_end_matches('/').rsplit('/').next().unwrap_or("Calendar").to_string()),
                color: ok.text_of("calendar-color"),
                ctag: ok.text_of("getctag"),
                sync_token: ok.text_of("sync-token"),
                read_only,
                href,
            });
        }
        Ok(out)
    }

    /// RFC 6578 incremental sync. `token = None` returns everything.
    pub async fn sync_collection(&self, cal_href: &str, token: Option<&str>) -> DavResult<Changes> {
        let token_xml = token.map(xml::escape).unwrap_or_default();
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:sync-collection xmlns:d="DAV:"><d:sync-token>{token_xml}</d:sync-token><d:sync-level>1</d:sync-level><d:prop><d:getetag/></d:prop></d:sync-collection>"#
        );
        let root = self.xml_request("REPORT", self.url(cal_href)?, "1", body).await?;
        let mut changes = Changes { token: root.text_of("sync-token"), ..Default::default() };
        let mut resps = Vec::new();
        root.find_all("response", &mut resps);
        let cal_url = self.url(cal_href)?;
        for r in resps {
            let Some(href) = r.text_of("href") else { continue };
            if self.url(&href).ok().as_ref() == Some(&cal_url) {
                continue;
            }
            let status = r.children_named("status").next().map(|s| s.text.clone()).unwrap_or_default();
            if status.contains("404") {
                changes.deleted.push(href);
            } else if href.ends_with(".ics") || r.find("getetag").is_some() {
                changes.changed.push((href, r.text_of("getetag")));
            }
        }
        Ok(changes)
    }

    /// Fallback: all (href, etag) pairs in a calendar.
    pub async fn list_etags(&self, cal_href: &str) -> DavResult<Vec<(String, Option<String>)>> {
        let resps = self.propfind(self.url(cal_href)?, "1", "<d:getetag/><d:resourcetype/>").await?;
        let cal_url = self.url(cal_href)?;
        Ok(resps
            .into_iter()
            .filter_map(|r| {
                let href = r.text_of("href")?;
                if self.url(&href).ok().as_ref() == Some(&cal_url) || r.find("collection").is_some() {
                    return None;
                }
                Some((href, r.text_of("getetag")))
            })
            .collect())
    }

    pub async fn ctag(&self, cal_href: &str) -> DavResult<Option<String>> {
        let resps = self.propfind(self.url(cal_href)?, "0", "<cs:getctag/><d:sync-token/>").await?;
        Ok(resps.iter().find_map(|r| r.text_of("getctag").or_else(|| r.text_of("sync-token"))))
    }

    /// Fetch objects. Returns (href, etag, ics).
    pub async fn multiget(&self, cal_href: &str, hrefs: &[String]) -> DavResult<Vec<(String, Option<String>, String)>> {
        let mut out = Vec::new();
        for chunk in hrefs.chunks(100) {
            let mut list = String::new();
            for h in chunk {
                list.push_str(&format!("<d:href>{}</d:href>", xml::escape(h)));
            }
            let body = format!(
                r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop>{list}</c:calendar-multiget>"#
            );
            let root = self.xml_request("REPORT", self.url(cal_href)?, "1", body).await?;
            let mut resps = Vec::new();
            root.find_all("response", &mut resps);
            for r in resps {
                if let (Some(href), Some(data)) = (r.text_of("href"), r.text_of("calendar-data")) {
                    out.push((href, r.text_of("getetag"), data));
                }
            }
        }
        Ok(out)
    }

    /// Upload. `etag = None` means "create" (If-None-Match: *). Returns the new ETag if given.
    pub async fn put(&self, href: &str, ics: &str, etag: Option<&str>) -> DavResult<Option<String>> {
        let headers: Vec<(&str, &str)> = match etag {
            Some(e) => vec![("If-Match", e)],
            None => vec![("If-None-Match", "*")],
        };
        let resp = self.send("PUT", self.url(href)?, None, Some(ics.to_string()), &headers).await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(DavError::Http(format!("{status} {}", text.chars().take(200).collect::<String>())));
        }
        Ok(resp.headers().get(header::ETAG).and_then(|v| v.to_str().ok()).map(String::from))
    }

    pub async fn delete(&self, href: &str, etag: Option<&str>) -> DavResult<()> {
        let headers: Vec<(&str, &str)> = etag.map(|e| vec![("If-Match", e)]).unwrap_or_default();
        match self.send("DELETE", self.url(href)?, None, None, &headers).await {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(resp) => Err(DavError::Http(resp.status().to_string())),
            Err(DavError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }
}
