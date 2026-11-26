//! Single instance: the first process listens on a localhost port recorded in
//! the data dir; later launches poke it ("show", plus any files to open) and exit.
//!
//! Protocol: `calen-show\n`, then one `open <path>\n` line per file, then the
//! client closes its write side and waits for `ok`. Older instances only read
//! the first 11 bytes, so they still show themselves (and ignore the files).

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{PlatformEvent, emit};

const TOKEN: &[u8] = b"calen-show\n";
const MAX_REQUEST: u64 = 64 * 1024;

/// Returns false if another instance is running (and was told to show itself
/// and open `files`).
pub fn acquire(data_dir: &Path, files: &[PathBuf]) -> bool {
    let port_file = data_dir.join("instance.port");
    if let Ok(s) = std::fs::read_to_string(&port_file)
        && let Ok(port) = s.trim().parse::<u16>()
    {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
            let mut request = TOKEN.to_vec();
            for f in files {
                request.extend_from_slice(format!("open {}\n", f.to_string_lossy()).as_bytes());
            }
            let _ = stream.write_all(&request);
            let _ = stream.shutdown(Shutdown::Write);
            let mut ack = [0u8; 2];
            let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
            if stream.read_exact(&mut ack).is_ok() && &ack == b"ok" {
                return false;
            }
        }
    }
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else { return true };
    if let Ok(addr) = listener.local_addr() {
        let _ = std::fs::write(&port_file, addr.port().to_string());
    }
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut buf = Vec::new();
            // Read to EOF (or the timeout, for clients that don't close their side).
            let _ = (&mut stream).take(MAX_REQUEST).read_to_end(&mut buf);
            let Some(rest) = buf.strip_prefix(TOKEN) else { continue };
            let _ = stream.write_all(b"ok");
            emit(PlatformEvent::SecondInstance);
            let files = parse_files(rest);
            if !files.is_empty() {
                emit(PlatformEvent::OpenFiles(files));
            }
        }
    });
    true
}

fn parse_files(rest: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(rest).lines().filter_map(|l| l.strip_prefix("open ")).filter(|p| !p.is_empty()).map(PathBuf::from).collect()
}

/// Calendar files named on the command line: `file://` URIs (what a desktop
/// entry's `%U` passes) or paths ending in `.ics`. Anything else (flags and
/// their values, other URIs) is ignored.
pub fn files_from_args(args: &[String]) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter_map(|a| match a.strip_prefix("file://") {
            Some(rest) => {
                // file:///path or file://localhost/path
                let path = rest.strip_prefix("localhost").unwrap_or(rest);
                path.starts_with('/').then(|| PathBuf::from(uri_path(&percent_decode(path))))
            }
            None if is_calendar_file(a) && !a.contains("://") => Some(PathBuf::from(a)),
            None => None,
        })
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect()
}

/// The local path of a `file://` URI's path part. On Windows, `/C:/x.ics`
/// is `C:/x.ics` (Windows accepts `/` as a separator).
fn uri_path(path: &str) -> &str {
    let b = path.as_bytes();
    if cfg!(windows) && b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { &path[1..] } else { path }
}

pub fn is_calendar_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".ics", ".ical", ".icalendar"].iter().any(|ext| lower.ends_with(ext))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// What the function makes of a path: absolute in the platform's terms
    /// (on Windows, `/tmp/a.ics` is on the current drive).
    fn abs(p: &str) -> PathBuf {
        std::path::absolute(p).unwrap()
    }

    #[test]
    fn args_to_files() {
        let a = args(&[
            "calen",
            "--minimized",
            "--demo",
            "shot:week:/tmp/w.png",
            "/tmp/a b.ics",
            "file:///tmp/inv%20ite.ics",
            "file://localhost/x.ics",
            "https://e.com/c.ics",
        ]);
        assert_eq!(files_from_args(&a), vec![abs("/tmp/a b.ics"), abs("/tmp/inv ite.ics"), abs("/x.ics")]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_uris() {
        let a = args(&["calen", "file:///C:/Users/Sam/My%20invite.ics"]);
        assert_eq!(files_from_args(&a), vec![PathBuf::from(r"C:\Users\Sam\My invite.ics")]);
    }

    #[test]
    fn request_lines() {
        assert_eq!(parse_files(b"open /a.ics\nopen /b c.ics\n"), vec![PathBuf::from("/a.ics"), PathBuf::from("/b c.ics")]);
        assert!(parse_files(b"").is_empty());
    }
}
