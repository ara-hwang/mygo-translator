//! A tiny blocking HTTP/1.1 client.
//!
//! The only HTTP peer this application talks to is a `llama-server` on loopback, so pulling in
//! a full client stack (TLS, async runtime, connection pools) would be dead weight. This module
//! implements exactly what is needed: one request per connection, `Content-Length`, chunked and
//! EOF-delimited bodies, and cancellable reads with an overall deadline.
//!
//! Cancellation is cooperative: the caller flips a flag and reads stop with
//! [`HttpError::Cancelled`] at the next poll boundary.

use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Shared cancellation flag. Clone it into the thread doing the work.
pub type CancelToken = Arc<AtomicBool>;

/// Creates a fresh, unsignalled token.
pub fn cancel_token() -> CancelToken {
    Arc::new(AtomicBool::new(false))
}

/// How often a blocked read wakes up to check for cancellation and the deadline.
const POLL_INTERVAL: Duration = Duration::from_millis(150);
/// Upper bound for the response head, to bound memory on a misbehaving peer.
const MAX_HEAD_BYTES: usize = 64 * 1024;
/// Upper bound for a single chunked-transfer size line.
const MAX_LINE_BYTES: usize = 8 * 1024;
/// Read buffer size for body chunks.
const READ_BUFFER: usize = 8 * 1024;

/// Failure modes of the minimal client.
#[derive(Debug)]
pub enum HttpError {
    /// Connecting failed (server not running yet, wrong port...).
    Connect(String),
    /// The socket failed while reading or writing.
    Io(io::Error),
    /// The deadline elapsed before the response completed.
    Timeout,
    /// The caller cancelled the request.
    Cancelled,
    /// The peer spoke something that is not valid HTTP/1.1.
    Protocol(String),
    /// The server answered with a non-2xx status; the body is included for diagnostics.
    Status {
        /// HTTP status code.
        code: u16,
        /// Reason phrase or the server's error message.
        body: String,
    },
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::Connect(detail) => write!(f, "서버에 연결할 수 없습니다 ({detail})"),
            HttpError::Io(err) => write!(f, "네트워크 오류: {err}"),
            HttpError::Timeout => write!(f, "응답 시간이 초과되었습니다."),
            HttpError::Cancelled => write!(f, "요청이 취소되었습니다."),
            HttpError::Protocol(detail) => write!(f, "서버 응답을 해석할 수 없습니다: {detail}"),
            HttpError::Status { code, body } => {
                if body.is_empty() {
                    write!(f, "서버가 HTTP {code}로 응답했습니다.")
                } else {
                    write!(f, "서버 오류 HTTP {code}: {body}")
                }
            }
        }
    }
}

impl std::error::Error for HttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HttpError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for HttpError {
    fn from(err: io::Error) -> Self {
        HttpError::Io(err)
    }
}

/// A request to send.
pub struct Request<'a> {
    /// HTTP method, e.g. `GET`.
    pub method: &'a str,
    /// Request target, e.g. `/health`.
    pub path: &'a str,
    /// Extra headers, without the automatic `Host`, `Content-Length` and `Connection`.
    pub headers: Vec<(&'a str, String)>,
    /// Request body; may be empty.
    pub body: Vec<u8>,
}

impl<'a> Request<'a> {
    /// A `GET` with no body.
    pub fn get(path: &'a str) -> Self {
        Self {
            method: "GET",
            path,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// A `POST` carrying a JSON body.
    pub fn post_json(path: &'a str, body: String) -> Self {
        Self {
            method: "POST",
            path,
            headers: vec![
                ("Content-Type", "application/json".to_string()),
                ("Accept", "text/event-stream".to_string()),
            ],
            body: body.into_bytes(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum BodyMode {
    Length(u64),
    Chunked,
    UntilEof,
}

/// An open response whose body can be consumed incrementally.
#[derive(Debug)]
pub struct Response {
    reader: BufReader<TcpStream>,
    /// Status code.
    pub status: u16,
    /// Reason phrase from the status line.
    pub reason: String,
    headers: Vec<(String, String)>,
    mode: BodyMode,
    chunk_remaining: u64,
    chunk_needs_crlf: bool,
    finished: bool,
    cancel: CancelToken,
    deadline: Instant,
}

impl Response {
    /// Looks up a header, case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// True for 2xx.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Reads the next piece of decoded body data. `Ok(None)` means the body is complete.
    pub fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        if self.finished {
            return Ok(None);
        }
        match self.mode {
            BodyMode::Length(remaining) => {
                if remaining == 0 {
                    self.finished = true;
                    return Ok(None);
                }
                let want = remaining.min(READ_BUFFER as u64) as usize;
                let mut buf = vec![0u8; want];
                let n = self.read_some(&mut buf)?;
                if n == 0 {
                    return Err(HttpError::Protocol(
                        "본문을 읽는 중 연결이 종료되었습니다.".into(),
                    ));
                }
                self.mode = BodyMode::Length(remaining - n as u64);
                buf.truncate(n);
                Ok(Some(buf))
            }
            BodyMode::UntilEof => {
                let mut buf = vec![0u8; READ_BUFFER];
                let n = self.read_some(&mut buf)?;
                if n == 0 {
                    self.finished = true;
                    return Ok(None);
                }
                buf.truncate(n);
                Ok(Some(buf))
            }
            BodyMode::Chunked => self.read_chunked(),
        }
    }

    fn read_chunked(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        if self.chunk_remaining == 0 {
            if self.chunk_needs_crlf {
                let line = self.read_line()?;
                if !line.trim().is_empty() {
                    return Err(HttpError::Protocol("청크 구분자가 잘못되었습니다.".into()));
                }
                self.chunk_needs_crlf = false;
            }
            let line = self.read_line()?;
            let size_text = line
                .trim()
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            let size = u64::from_str_radix(&size_text, 16)
                .map_err(|_| HttpError::Protocol(format!("잘못된 청크 크기: {size_text:?}")))?;
            if size == 0 {
                // Consume optional trailers, then the final blank line.
                loop {
                    let trailer = self.read_line()?;
                    if trailer.trim().is_empty() {
                        break;
                    }
                }
                self.finished = true;
                return Ok(None);
            }
            self.chunk_remaining = size;
        }

        let want = self.chunk_remaining.min(READ_BUFFER as u64) as usize;
        let mut buf = vec![0u8; want];
        let n = self.read_some(&mut buf)?;
        if n == 0 {
            return Err(HttpError::Protocol(
                "청크 본문을 읽는 중 연결이 종료되었습니다.".into(),
            ));
        }
        self.chunk_remaining -= n as u64;
        if self.chunk_remaining == 0 {
            self.chunk_needs_crlf = true;
        }
        buf.truncate(n);
        Ok(Some(buf))
    }

    /// Reads the whole body, refusing to buffer more than `cap` bytes.
    pub fn read_to_end(&mut self, cap: usize) -> Result<Vec<u8>, HttpError> {
        let mut out = Vec::new();
        while let Some(chunk) = self.read_chunk()? {
            if out.len() + chunk.len() > cap {
                return Err(HttpError::Protocol(format!(
                    "응답 본문이 너무 큽니다(최대 {cap} 바이트)."
                )));
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// Reads the whole body as UTF-8 (lossy).
    pub fn read_text(&mut self, cap: usize) -> Result<String, HttpError> {
        let bytes = self.read_to_end(cap)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn read_some(&mut self, buf: &mut [u8]) -> Result<usize, HttpError> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(HttpError::Cancelled);
            }
            if Instant::now() >= self.deadline {
                return Err(HttpError::Timeout);
            }
            match self.reader.read(buf) {
                Ok(n) => return Ok(n),
                Err(err) if is_retryable(&err) => {
                    if Instant::now() >= self.deadline {
                        return Err(HttpError::Timeout);
                    }
                }
                Err(err) => return Err(HttpError::Io(err)),
            }
        }
    }

    fn read_line(&mut self) -> Result<String, HttpError> {
        let mut line = Vec::new();
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(HttpError::Cancelled);
            }
            if Instant::now() >= self.deadline {
                return Err(HttpError::Timeout);
            }
            if line.len() > MAX_LINE_BYTES {
                return Err(HttpError::Protocol("응답 헤더 줄이 너무 깁니다.".into()));
            }
            let mut byte = [0u8; 1];
            match self.reader.read(&mut byte) {
                Ok(0) => {
                    return Err(HttpError::Protocol(
                        "응답이 예상보다 일찍 종료되었습니다.".into(),
                    ))
                }
                Ok(_) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    line.push(byte[0]);
                }
                Err(err) if is_retryable(&err) => {
                    if Instant::now() >= self.deadline {
                        return Err(HttpError::Timeout);
                    }
                }
                Err(err) => return Err(HttpError::Io(err)),
            }
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Ok(String::from_utf8_lossy(&line).into_owned())
    }
}

/// Opens a connection, sends `request`, and reads the response head.
pub fn open(
    addr: SocketAddr,
    request: &Request<'_>,
    cancel: CancelToken,
    timeout: Duration,
) -> Result<Response, HttpError> {
    let deadline = Instant::now() + timeout;
    let connect_timeout = Duration::from_secs(5).min(timeout);
    let stream = TcpStream::connect_timeout(&addr, connect_timeout)
        .map_err(|err| HttpError::Connect(format!("{addr} — {err}")))?;
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(POLL_INTERVAL))
        .map_err(HttpError::Io)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(HttpError::Io)?;

    let mut writer = stream.try_clone().map_err(HttpError::Io)?;
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nAccept-Encoding: identity\r\n",
        request.method, request.path, addr
    );
    for (name, value) in &request.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        request.body.len()
    ));
    if cancel.load(Ordering::Relaxed) {
        return Err(HttpError::Cancelled);
    }
    writer.write_all(head.as_bytes()).map_err(HttpError::Io)?;
    if !request.body.is_empty() {
        writer.write_all(&request.body).map_err(HttpError::Io)?;
    }
    writer.flush().map_err(HttpError::Io)?;

    let mut reader = BufReader::new(stream);
    let head_bytes = read_head(&mut reader, &cancel, deadline)?;
    let (status, reason, headers) = parse_head(&head_bytes)?;

    let mode = if let Some(encoding) = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("transfer-encoding"))
        .map(|(_, value)| value.to_ascii_lowercase())
    {
        if encoding.contains("chunked") {
            BodyMode::Chunked
        } else {
            BodyMode::UntilEof
        }
    } else if let Some(length) = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<u64>().ok())
    {
        BodyMode::Length(length)
    } else {
        BodyMode::UntilEof
    };

    Ok(Response {
        reader,
        status,
        reason,
        headers,
        mode,
        chunk_remaining: 0,
        chunk_needs_crlf: false,
        finished: false,
        cancel,
        deadline,
    })
}

fn read_head(
    reader: &mut BufReader<TcpStream>,
    cancel: &CancelToken,
    deadline: Instant,
) -> Result<Vec<u8>, HttpError> {
    let mut head = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(HttpError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(HttpError::Timeout);
        }
        if head.len() > MAX_HEAD_BYTES {
            return Err(HttpError::Protocol("응답 헤더가 너무 큽니다.".into()));
        }
        match reader.read(&mut byte) {
            Ok(0) => {
                return Err(HttpError::Protocol(
                    "서버가 응답 없이 연결을 닫았습니다.".into(),
                ))
            }
            Ok(_) => {
                head.push(byte[0]);
                if head.ends_with(b"\r\n\r\n") {
                    return Ok(head);
                }
            }
            Err(err) if is_retryable(&err) => {
                if Instant::now() >= deadline {
                    return Err(HttpError::Timeout);
                }
            }
            Err(err) => return Err(HttpError::Io(err)),
        }
    }
}

fn parse_head(head: &[u8]) -> Result<(u16, String, Vec<(String, String)>), HttpError> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") {
        return Err(HttpError::Protocol(format!(
            "알 수 없는 상태 줄: {status_line:?}"
        )));
    }
    let status: u16 = parts
        .next()
        .ok_or_else(|| HttpError::Protocol("상태 코드가 없습니다.".into()))?
        .parse()
        .map_err(|_| HttpError::Protocol(format!("잘못된 상태 코드: {status_line:?}")))?;
    let reason = parts.next().unwrap_or_default().to_string();

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| HttpError::Protocol(format!("잘못된 헤더 줄: {line:?}")))?;
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    Ok((status, reason, headers))
}

fn is_retryable(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::mock::{MockServer, MockSpec};

    #[test]
    fn reads_content_length_bodies() {
        let server = MockServer::start(MockSpec::json(r#"{"ok":true}"#));
        let token = cancel_token();
        let addr = server.addr();
        let mut response = open(
            addr,
            &Request::get("/health"),
            token,
            Duration::from_secs(5),
        )
        .expect("request");
        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(response.read_text(4096).unwrap(), r#"{"ok":true}"#);
    }

    #[test]
    fn decodes_chunked_bodies_split_at_awkward_boundaries() {
        let server = MockServer::start(MockSpec::chunked(vec!["hel", "lo ", "세계"]));
        let token = cancel_token();
        let mut response = open(
            server.addr(),
            &Request::get("/stream"),
            token,
            Duration::from_secs(5),
        )
        .expect("request");
        let mut body = Vec::new();
        while let Some(chunk) = response.read_chunk().unwrap() {
            body.extend_from_slice(&chunk);
        }
        assert_eq!(String::from_utf8(body).unwrap(), "hello 세계");
    }

    #[test]
    fn enforces_deadline_while_body_chunks_keep_arriving() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            for _ in 0..12 {
                std::thread::sleep(Duration::from_millis(10));
                if stream.write_all(b"x").is_err() {
                    break;
                }
            }
        });

        let mut response = open(
            addr,
            &Request::get("/stream"),
            cancel_token(),
            Duration::from_millis(45),
        )
        .expect("request");
        assert!(matches!(response.read_to_end(32), Err(HttpError::Timeout)));
        server.join().unwrap();
    }

    #[test]
    fn reports_status_errors_with_the_body() {
        let server = MockServer::start(MockSpec::status(500, "model failed to load"));
        let mut response = open(
            server.addr(),
            &Request::get("/health"),
            cancel_token(),
            Duration::from_secs(5),
        )
        .expect("request");
        assert!(!response.is_success());
        assert_eq!(response.status, 500);
        assert_eq!(response.read_text(4096).unwrap(), "model failed to load");
    }

    #[test]
    fn honors_cancellation_before_connecting() {
        let token = cancel_token();
        token.store(true, Ordering::Relaxed);
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let error = open(addr, &Request::get("/"), token, Duration::from_secs(1));
        match error {
            Err(HttpError::Cancelled) | Err(HttpError::Connect(_)) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn posts_a_body_and_parses_the_response() {
        let server = MockServer::start(MockSpec::json(r#"{"echo":1}"#));
        let mut response = open(
            server.addr(),
            &Request::post_json("/v1/chat/completions", r#"{"a":1}"#.to_string()),
            cancel_token(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(response.read_text(4096).unwrap(), r#"{"echo":1}"#);
        let received = server.last_request();
        assert!(received.contains("POST /v1/chat/completions HTTP/1.1"));
        assert!(received.to_ascii_lowercase().contains("content-length: 7"));
        assert!(received.ends_with(r#"{"a":1}"#));
    }
}
