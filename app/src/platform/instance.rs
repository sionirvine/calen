//! Single instance: the first process listens on a localhost port recorded in
//! the data dir; later launches poke it ("show") and exit.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use super::{PlatformEvent, emit};

const TOKEN: &[u8] = b"calen-show\n";

/// Returns false if another instance is running (and was told to show itself).
pub fn acquire(data_dir: &Path) -> bool {
    let port_file = data_dir.join("instance.port");
    if let Ok(s) = std::fs::read_to_string(&port_file)
        && let Ok(port) = s.trim().parse::<u16>()
    {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
            let _ = stream.write_all(TOKEN);
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
            let mut buf = [0u8; 11];
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            if stream.read_exact(&mut buf).is_ok() && buf == TOKEN {
                let _ = stream.write_all(b"ok");
                emit(PlatformEvent::SecondInstance);
            }
        }
    });
    true
}
