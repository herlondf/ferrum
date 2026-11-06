//! A tiny HTTP server for tests: answers each request from a route table
//! and records what it received. Never talks to real services.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Received {
    pub method: String,
    /// Path and query.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub struct Mock {
    pub base: String,
    pub received: Arc<Mutex<Vec<Received>>>,
}

/// Starts a server. `route` gets (method, path) and returns status, extra
/// headers and the JSON body.
pub fn serve(
    route: impl Fn(&str, &str) -> (u16, Vec<(&'static str, String)>, String) + Send + 'static,
) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let received = Arc::new(Mutex::new(Vec::new()));
    let log = received.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            let mut headers = Vec::new();
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = h.trim_end().split_once(':') {
                    headers.push((k.trim().to_owned(), v.trim().to_owned()));
                }
            }
            let len = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = vec![0; len];
            let _ = reader.read_exact(&mut body);
            let (status, extra, reply) = route(&method, &path);
            log.lock().unwrap().push(Received {
                method,
                path,
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
            });
            let mut out = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                reply.len()
            );
            for (k, v) in extra {
                out.push_str(&format!("{k}: {v}\r\n"));
            }
            out.push_str("\r\n");
            out.push_str(&reply);
            let _ = stream.write_all(out.as_bytes());
        }
    });
    Mock { base, received }
}

impl Mock {
    pub fn last(&self) -> Received {
        self.received
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a request")
    }

    pub fn all(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }
}
