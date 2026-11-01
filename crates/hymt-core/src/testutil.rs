//! Test-only helpers: temporary directories and a tiny mock HTTP server.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A directory that removes itself when dropped.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a uniquely named directory under the system temp folder.
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("hymt-test-{}-{label}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    /// The directory path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Canned responses for the mock server.
pub mod mock {
    use super::*;

    /// What the next connection should answer with.
    #[derive(Clone)]
    pub enum MockSpec {
        /// `200 OK` with a JSON string body and `Content-Length`.
        Json(String),
        /// A specific status code and body.
        Status(u16, String),
        /// `200 OK` with `Transfer-Encoding: chunked`; each entry is one chunk.
        Chunked(Vec<String>),
        /// `200 OK` SSE stream; each entry becomes one `data:` line.
        Sse(Vec<String>),
        /// Serves the entries in order, repeating the last one.
        Sequence(Vec<MockSpec>),
    }

    impl MockSpec {
        /// A JSON response.
        pub fn json(body: &str) -> Self {
            MockSpec::Json(body.to_string())
        }

        /// An error status with a plain-text body.
        pub fn status(code: u16, body: &str) -> Self {
            MockSpec::Status(code, body.to_string())
        }

        /// A chunked response.
        pub fn chunked(parts: Vec<&str>) -> Self {
            MockSpec::Chunked(parts.into_iter().map(str::to_string).collect())
        }

        /// An SSE response.
        pub fn sse(events: Vec<&str>) -> Self {
            MockSpec::Sse(events.into_iter().map(str::to_string).collect())
        }
    }

    /// A loopback server that records the last request it saw.
    pub struct MockServer {
        addr: SocketAddr,
        last_request: Arc<Mutex<String>>,
        shutdown: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl MockServer {
        /// Starts the server on an ephemeral port.
        pub fn start(spec: MockSpec) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
            let addr = listener.local_addr().expect("local addr");
            let last_request = Arc::new(Mutex::new(String::new()));
            let shutdown = Arc::new(AtomicBool::new(false));
            let spec = Arc::new(spec);
            let hits = Arc::new(AtomicUsize::new(0));
            let thread_state = (listener, spec, hits, last_request.clone(), shutdown.clone());
            let handle = thread::spawn(move || {
                let (listener, spec, hits, last_request, shutdown) = thread_state;
                listener
                    .set_nonblocking(true)
                    .expect("non-blocking listener");
                while !shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                            let request = read_request(&mut stream);
                            if let Some(mut guard) = last_request.try_lock().ok() {
                                guard.clear();
                                guard.push_str(&request);
                            }
                            let index = hits.fetch_add(1, Ordering::Relaxed);
                            write_response(&mut stream, spec.resolve(index));
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                addr,
                last_request,
                shutdown,
                handle: Some(handle),
            }
        }

        /// Address to connect to.
        pub fn addr(&self) -> SocketAddr {
            self.addr
        }

        /// The raw head and body of the most recent request.
        pub fn last_request(&self) -> String {
            self.last_request.lock().expect("lock").clone()
        }
    }

    impl MockSpec {
        fn resolve(&self, index: usize) -> &MockSpec {
            match self {
                MockSpec::Sequence(items) => items
                    .get(index)
                    .unwrap_or_else(|| items.last().expect("non-empty")),
                other => other,
            }
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match stream.read(&mut byte) {
                Ok(0) | Err(_) => return String::from_utf8_lossy(&buffer).into_owned(),
                Ok(_) => {
                    buffer.push(byte[0]);
                    if buffer.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }
        let head = String::from_utf8_lossy(&buffer).into_owned();
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        let mut filled = 0;
        while filled < length {
            match stream.read(&mut body[filled..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => filled += n,
            }
        }
        let mut full = head;
        full.push_str(&String::from_utf8_lossy(&body[..filled]));
        full
    }

    fn write_response(stream: &mut TcpStream, spec: &MockSpec) {
        match spec {
            MockSpec::Sequence(_) => {}
            MockSpec::Json(body) => {
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
            MockSpec::Status(code, body) => {
                let head = format!(
                    "HTTP/1.1 {code} Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
            MockSpec::Chunked(parts) => {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                );
                for part in parts {
                    let mut frame = format!("{:x}\r\n", part.len());
                    frame.push_str(part);
                    frame.push_str("\r\n");
                    let _ = stream.write_all(frame.as_bytes());
                    let _ = stream.flush();
                    thread::sleep(Duration::from_millis(2));
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            }
            MockSpec::Sse(events) => {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                );
                for event in events {
                    let frame_body = format!("data: {event}\n\n");
                    let mut frame = format!("{:x}\r\n", frame_body.len());
                    frame.push_str(&frame_body);
                    frame.push_str("\r\n");
                    let _ = stream.write_all(frame.as_bytes());
                    let _ = stream.flush();
                    thread::sleep(Duration::from_millis(5));
                }
                let _ = stream.write_all(b"0\r\n\r\n");
                let _ = stream.flush();
            }
        }
    }

    /// Convenience: build an SSE completion chunk for a delta string.
    pub fn delta_chunk(text: &str) -> String {
        let mut escaped = String::new();
        for ch in text.chars() {
            match ch {
                '"' => escaped.push_str("\\\""),
                '\\' => escaped.push_str("\\\\"),
                '\n' => escaped.push_str("\\n"),
                _ => escaped.push(ch),
            }
        }
        let mut out = String::new();
        write!(
            out,
            "{{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{escaped}\"}}}}]}}"
        )
        .unwrap();
        out
    }
}
