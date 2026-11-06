//! One Ferrum window per data folder: a second `ferrum PATH` hands its
//! requests to the running instance over a local socket and exits.
//!
//! Unix uses a socket file in the data folder. Windows listens on a
//! localhost TCP port written, with a random token, to a file there.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::repos::{Request, decode, encode};

/// Requests received from other processes.
pub struct Listener {
    rx: Receiver<Vec<Request>>,
    ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl Listener {
    /// Lets the listener wake the window up when a request arrives.
    pub fn set_context(&self, ctx: &egui::Context) {
        if let Ok(mut c) = self.ctx.lock()
            && c.is_none()
        {
            *c = Some(ctx.clone());
        }
    }

    pub fn poll(&self) -> Vec<Request> {
        self.rx.try_iter().flatten().collect()
    }
}

pub enum Start {
    /// Another instance took the requests; this process should exit.
    Forwarded,
    /// This is the first instance.
    Serve(Listener),
    /// No socket could be used: run on our own.
    Alone,
}

/// Disabled for tests, screenshots and with `FERRUM_NO_SINGLE_INSTANCE`.
pub fn enabled() -> bool {
    !cfg!(test)
        && std::env::var_os("FERRUM_NO_SINGLE_INSTANCE").is_none()
        && std::env::var_os("FERRUM_SCREENSHOT").is_none()
}

pub fn start(dir: &Path, requests: &[Request]) -> Start {
    let _ = std::fs::create_dir_all(dir);
    let message = encode(requests);
    if imp::send(dir, &message) {
        return Start::Forwarded;
    }
    let (tx, rx) = mpsc::channel();
    let ctx = Arc::new(Mutex::new(None));
    if imp::listen(
        dir,
        Handler {
            tx,
            ctx: ctx.clone(),
        },
    ) {
        Start::Serve(Listener { rx, ctx })
    } else {
        Start::Alone
    }
}

#[derive(Clone)]
struct Handler {
    tx: Sender<Vec<Request>>,
    ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl Handler {
    fn receive(&self, mut stream: impl Read) {
        let mut text = String::new();
        // A client sends a few lines; cap what is read from anyone else.
        if stream
            .by_ref()
            .take(64 * 1024)
            .read_to_string(&mut text)
            .is_err()
        {
            return;
        }
        let _ = self.tx.send(decode(&text));
        if let Ok(c) = self.ctx.lock()
            && let Some(ctx) = c.as_ref()
        {
            ctx.request_repaint();
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    fn socket(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("instance.sock");
        // Socket paths are limited to about 100 bytes: use the runtime folder
        // with a name derived from the data folder instead.
        if path.as_os_str().len() < 100 {
            return path;
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        dir.hash(&mut h);
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        runtime.join(format!("ferrum-{:016x}.sock", h.finish()))
    }

    pub fn send(dir: &Path, message: &str) -> bool {
        let Ok(mut s) = UnixStream::connect(socket(dir)) else {
            return false;
        };
        let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
        s.write_all(message.as_bytes()).is_ok()
    }

    pub fn listen(dir: &Path, handler: Handler) -> bool {
        let path = socket(dir);
        // Nobody answered: the file is left over from a crash.
        let _ = std::fs::remove_file(&path);
        let Ok(listener) = UnixListener::bind(&path) else {
            return false;
        };
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                handler.receive(stream);
            }
        });
        true
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;
    use std::hash::{BuildHasher, Hasher};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};

    fn info_file(dir: &Path) -> std::path::PathBuf {
        dir.join("instance.port")
    }

    pub fn send(dir: &Path, message: &str) -> bool {
        let Ok(info) = std::fs::read_to_string(info_file(dir)) else {
            return false;
        };
        let Some((port, token)) = info.trim().split_once(' ') else {
            return false;
        };
        let Ok(port) = port.parse::<u16>() else {
            return false;
        };
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
            return false;
        };
        let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
        s.write_all(format!("{token}\n{message}").as_bytes())
            .is_ok()
    }

    fn random_token() -> String {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u32(std::process::id());
        let a = h.finish();
        let b = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        format!("{a:016x}{b:016x}")
    }

    pub fn listen(dir: &Path, handler: Handler) -> bool {
        let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else {
            return false;
        };
        let Ok(addr) = listener.local_addr() else {
            return false;
        };
        let token = random_token();
        if std::fs::write(info_file(dir), format!("{} {token}", addr.port())).is_err() {
            return false;
        }
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                // Only processes that can read the data folder know the token.
                let mut first = Vec::new();
                let mut byte = [0u8; 1];
                while first.len() < 128 && stream.read_exact(&mut byte).is_ok() && byte[0] != b'\n'
                {
                    first.push(byte[0]);
                }
                if first == token.as_bytes() {
                    handler.receive(stream);
                }
            }
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_start_forwards_to_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let Start::Serve(listener) = start(dir.path(), &[]) else {
            panic!("first start should listen");
        };
        let reqs = [Request::Open("/some/repo".into())];
        assert!(matches!(start(dir.path(), &reqs), Start::Forwarded));
        let mut got = Vec::new();
        for _ in 0..200 {
            got.extend(listener.poll());
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(got, reqs);
    }
}
