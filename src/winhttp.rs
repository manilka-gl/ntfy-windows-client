use crate::protocol::{Message, parse_line, sanitize_header, valid_topic};
use std::{
    ffi::c_void,
    fmt,
    mem::size_of,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    errhandlingapi::GetLastError,
    winhttp::{
        HINTERNET, HTTP_STATUS_OK, INTERNET_DEFAULT_HTTP_PORT, INTERNET_DEFAULT_HTTPS_PORT,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
        WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    },
};

const USER_AGENT: &str = concat!("ntfy-windows-client/", env!("CARGO_PKG_VERSION"));
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_PUBLISH_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub server_url: String,
    pub topic: String,
    pub token: String,
}

#[derive(Clone, Debug)]
pub enum Event {
    Status(String),
    Connected(bool),
    Message(Message),
    Published,
    Error(String),
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub struct Controller {
    stop: Arc<AtomicBool>,
    active_request: Arc<AtomicPtr<c_void>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            active_request: Arc::new(AtomicPtr::new(ptr::null_mut())),
            thread: None,
        }
    }
}

impl Controller {
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.thread.is_some()
    }

    pub fn start<F>(&mut self, config: ClientConfig, on_event: F) -> Result<(), Error>
    where
        F: Fn(Event) + Send + 'static,
    {
        validate_config(&config)?;
        self.stop();
        self.stop = Arc::new(AtomicBool::new(false));
        self.active_request = Arc::new(AtomicPtr::new(ptr::null_mut()));
        let stop = Arc::clone(&self.stop);
        let active_request = Arc::clone(&self.active_request);
        let callback: Box<dyn Fn(Event) + Send> = Box::new(on_event);
        let handle = thread::Builder::new()
            .name("ntfy-subscription".into())
            .stack_size(320 * 1024)
            .spawn(move || run_subscription(config, stop, active_request, callback))
            .map_err(|error| Error(format!("could not start network thread: {error}")))?;
        self.thread = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let request = self.active_request.swap(ptr::null_mut(), Ordering::AcqRel);
        if !request.is_null() {
            unsafe {
                WinHttpCloseHandle(request);
            }
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn publish<F>(
    config: ClientConfig,
    title: String,
    body: String,
    on_event: F,
) -> Result<(), Error>
where
    F: FnOnce(Event) + Send + 'static,
{
    validate_config(&config)?;
    if body.trim().is_empty() {
        return Err(Error("message cannot be empty".into()));
    }
    if body.len() > MAX_PUBLISH_BYTES {
        return Err(Error("message exceeds the 1 MiB safety limit".into()));
    }
    thread::Builder::new()
        .name("ntfy-publish".into())
        .stack_size(256 * 1024)
        .spawn(move || match publish_blocking(&config, &title, &body) {
            Ok(()) => on_event(Event::Published),
            Err(error) => on_event(Event::Error(error.to_string())),
        })
        .map(|_| ())
        .map_err(|error| Error(format!("could not start publish thread: {error}")))
}

fn validate_config(config: &ClientConfig) -> Result<(), Error> {
    if config.server_url.len() > 2048 {
        return Err(Error("server URL is too long".into()));
    }
    ParsedServer::parse(&config.server_url)?;
    if !valid_topic(config.topic.trim()) {
        return Err(Error(
            "topic must be 1-64 letters, digits, '-' or '_'".into(),
        ));
    }
    if config.token.contains(['\r', '\n', '\0']) {
        return Err(Error("token contains invalid characters".into()));
    }
    Ok(())
}

fn run_subscription(
    config: ClientConfig,
    stop: Arc<AtomicBool>,
    active_request: Arc<AtomicPtr<c_void>>,
    on_event: Box<dyn Fn(Event) + Send>,
) {
    let mut since = String::from("10m");
    let mut backoff_seconds = 1_u64;
    while !stop.load(Ordering::Acquire) {
        on_event(Event::Status("Connecting".into()));
        let since_query = since.clone();
        let result = subscribe_once(
            &config,
            &since_query,
            &stop,
            &active_request,
            &|| on_event(Event::Connected(true)),
            &mut |message| {
                if !message.id.is_empty() {
                    since.clone_from(&message.id);
                }
                on_event(Event::Message(message));
            },
        );

        on_event(Event::Connected(false));
        match result {
            Ok(()) => backoff_seconds = 1,
            Err(_) if stop.load(Ordering::Acquire) => break,
            Err(error) => {
                on_event(Event::Error(format!(
                    "{error}; reconnecting in {backoff_seconds}s"
                )));
                sleep_interruptibly(&stop, Duration::from_secs(backoff_seconds));
                backoff_seconds = (backoff_seconds * 2).min(30);
            }
        }
    }
    on_event(Event::Status("Disconnected".into()));
}

fn sleep_interruptibly(stop: &AtomicBool, duration: Duration) {
    let slices = duration.as_millis().div_ceil(100);
    for _ in 0..slices {
        if stop.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn subscribe_once<F>(
    config: &ClientConfig,
    since: &str,
    stop: &AtomicBool,
    active_request: &AtomicPtr<c_void>,
    on_connected: &dyn Fn(),
    on_message: &mut F,
) -> Result<(), Error>
where
    F: FnMut(Message),
{
    let server = ParsedServer::parse(&config.server_url)?;
    let object = format!(
        "{}/{}/json?since={}",
        server.base_path,
        config.topic.trim(),
        sanitize_header(since, 64)
    );
    let session = open_session(90_000)?;
    let host = wide(&server.host);
    let connect = Handle::new(unsafe { WinHttpConnect(session.0, host.as_ptr(), server.port, 0) })?;
    let method = wide("GET");
    let object = wide(&object);
    let request_raw = unsafe {
        WinHttpOpenRequest(
            connect.0,
            method.as_ptr(),
            object.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            if server.secure {
                WINHTTP_FLAG_SECURE as u32
            } else {
                0
            },
        )
    };
    let request = RequestHandle::new(request_raw, active_request)?;
    let headers = auth_headers(&config.token);
    let headers_wide = wide(&headers);
    winhttp_bool(
        unsafe {
            WinHttpSendRequest(
                request.raw,
                headers_wide.as_ptr(),
                headers_wide.len().saturating_sub(1) as u32,
                ptr::null(),
                0,
                0,
                0,
            )
        },
        "send subscription request",
    )?;
    winhttp_bool(
        unsafe { WinHttpReceiveResponse(request.raw, ptr::null_mut()) },
        "receive subscription response",
    )?;
    let status = query_status(request.raw)?;
    if status != HTTP_STATUS_OK as u32 {
        return Err(Error(format!("subscription returned HTTP {status}")));
    }
    if active_request.load(Ordering::Acquire) != request.raw {
        return Err(Error("subscription was cancelled".into()));
    }
    on_connected();

    let mut pending = Vec::<u8>::with_capacity(4096);
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let mut available = 0_u32;
        winhttp_bool(
            unsafe { WinHttpQueryDataAvailable(request.raw, &mut available) },
            "query subscription data",
        )?;
        if available == 0 {
            break;
        }
        let read_length = available.min(64 * 1024) as usize;
        let start = pending.len();
        pending.resize(start + read_length, 0);
        let mut read = 0_u32;
        winhttp_bool(
            unsafe {
                WinHttpReadData(
                    request.raw,
                    pending[start..].as_mut_ptr().cast::<c_void>(),
                    read_length as u32,
                    &mut read,
                )
            },
            "read subscription data",
        )?;
        pending.truncate(start + read as usize);
        consume_lines(&mut pending, on_message)?;
        if pending.len() > MAX_LINE_BYTES {
            return Err(Error("incoming ntfy event exceeded 1 MiB".into()));
        }
    }
    Ok(())
}

fn consume_lines<F>(pending: &mut Vec<u8>, on_message: &mut F) -> Result<(), Error>
where
    F: FnMut(Message),
{
    let mut consumed = 0;
    while let Some(relative) = pending[consumed..].iter().position(|byte| *byte == b'\n') {
        let end = consumed + relative;
        let line = &pending[consumed..end];
        if !line.is_empty() {
            if let Some(message) =
                parse_line(line).map_err(|error| Error(format!("invalid ntfy event: {error}")))?
            {
                on_message(message);
            }
        }
        consumed = end + 1;
    }
    if consumed > 0 {
        pending.drain(..consumed);
    }
    Ok(())
}

fn publish_blocking(config: &ClientConfig, title: &str, body: &str) -> Result<(), Error> {
    let server = ParsedServer::parse(&config.server_url)?;
    let object = format!("{}/{}", server.base_path, config.topic.trim());
    let session = open_session(30_000)?;
    let host = wide(&server.host);
    let connect = Handle::new(unsafe { WinHttpConnect(session.0, host.as_ptr(), server.port, 0) })?;
    let method = wide("POST");
    let object = wide(&object);
    let request = Handle::new(unsafe {
        WinHttpOpenRequest(
            connect.0,
            method.as_ptr(),
            object.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            if server.secure {
                WINHTTP_FLAG_SECURE as u32
            } else {
                0
            },
        )
    })?;
    let mut headers = auth_headers(&config.token);
    let title = sanitize_header(title.trim(), 256);
    if !title.is_empty() {
        headers.push_str("Title: ");
        headers.push_str(&title);
        headers.push_str("\r\n");
    }
    headers.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    let headers_wide = wide(&headers);
    let bytes = body.as_bytes();
    winhttp_bool(
        unsafe {
            WinHttpSendRequest(
                request.0,
                headers_wide.as_ptr(),
                headers_wide.len().saturating_sub(1) as u32,
                bytes.as_ptr().cast::<c_void>(),
                bytes.len() as u32,
                bytes.len() as u32,
                0,
            )
        },
        "send publish request",
    )?;
    winhttp_bool(
        unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) },
        "receive publish response",
    )?;
    let status = query_status(request.0)?;
    if !(200..300).contains(&status) {
        return Err(Error(format!("publish returned HTTP {status}")));
    }
    Ok(())
}

fn open_session(receive_timeout: i32) -> Result<Handle, Error> {
    let agent = wide(USER_AGENT);
    let session = Handle::new(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY as u32,
            ptr::null(),
            ptr::null(),
            0,
        )
    })?;
    winhttp_bool(
        unsafe { WinHttpSetTimeouts(session.0, 0, 15_000, 15_000, receive_timeout) },
        "set WinHTTP timeouts",
    )?;
    Ok(session)
}

fn query_status(request: HINTERNET) -> Result<u32, Error> {
    let mut status = 0_u32;
    let mut size = size_of::<u32>() as u32;
    winhttp_bool(
        unsafe {
            WinHttpQueryHeaders(
                request,
                (WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER) as u32,
                ptr::null(),
                (&raw mut status).cast::<c_void>(),
                &raw mut size,
                ptr::null_mut(),
            )
        },
        "query HTTP status",
    )?;
    Ok(status)
}

fn auth_headers(token: &str) -> String {
    if token.is_empty() {
        String::new()
    } else {
        format!("Authorization: Bearer {token}\r\n")
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn winhttp_bool(ok: i32, operation: &str) -> Result<(), Error> {
    if ok != 0 {
        Ok(())
    } else {
        let code = unsafe { GetLastError() };
        Err(Error(format!("{operation} failed (Windows error {code})")))
    }
}

struct RequestHandle<'a> {
    raw: HINTERNET,
    registry: &'a AtomicPtr<c_void>,
}

impl<'a> RequestHandle<'a> {
    fn new(raw: HINTERNET, registry: &'a AtomicPtr<c_void>) -> Result<Self, Error> {
        if raw.is_null() {
            let code = unsafe { GetLastError() };
            return Err(Error(format!(
                "WinHTTP handle creation failed (Windows error {code})"
            )));
        }
        registry.store(raw, Ordering::Release);
        Ok(Self { raw, registry })
    }
}

impl Drop for RequestHandle<'_> {
    fn drop(&mut self) {
        if self
            .registry
            .compare_exchange(
                self.raw,
                ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            unsafe {
                WinHttpCloseHandle(self.raw);
            }
        }
    }
}

struct Handle(HINTERNET);

impl Handle {
    fn new(raw: HINTERNET) -> Result<Self, Error> {
        if raw.is_null() {
            let code = unsafe { GetLastError() };
            Err(Error(format!(
                "WinHTTP handle creation failed (Windows error {code})"
            )))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
}

#[derive(Debug)]
struct ParsedServer {
    secure: bool,
    host: String,
    port: u16,
    base_path: String,
}

impl ParsedServer {
    fn parse(input: &str) -> Result<Self, Error> {
        let input = input.trim().trim_end_matches('/');
        let (secure, rest, default_port) = if let Some(rest) = input.strip_prefix("https://") {
            (true, rest, INTERNET_DEFAULT_HTTPS_PORT as u16)
        } else if let Some(rest) = input.strip_prefix("http://") {
            (false, rest, INTERNET_DEFAULT_HTTP_PORT as u16)
        } else {
            return Err(Error(
                "server URL must begin with http:// or https://".into(),
            ));
        };
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, ""), |(host, path)| (host, path));
        if authority.is_empty() || authority.contains(['@', '?', '#']) {
            return Err(Error("server URL has an invalid host".into()));
        }
        let (host, port) = parse_authority(authority, default_port)?;
        let base_path = if path.is_empty() {
            String::new()
        } else {
            format!("/{}", path.trim_matches('/'))
        };
        if base_path.contains(['?', '#']) {
            return Err(Error(
                "server URL path cannot contain query or fragment".into(),
            ));
        }
        Ok(Self {
            secure,
            host,
            port,
            base_path,
        })
    }
}

fn parse_authority(authority: &str, default_port: u16) -> Result<(String, u16), Error> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| Error("invalid IPv6 address".into()))?;
        let host = &rest[..end];
        let suffix = &rest[end + 1..];
        let port = if suffix.is_empty() {
            default_port
        } else {
            suffix
                .strip_prefix(':')
                .ok_or_else(|| Error("invalid server port".into()))?
                .parse::<u16>()
                .map_err(|_| Error("invalid server port".into()))?
        };
        return Ok((host.to_owned(), port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') {
            return Err(Error("IPv6 addresses must use brackets".into()));
        }
        let port = port
            .parse::<u16>()
            .map_err(|_| Error("invalid server port".into()))?;
        if host.is_empty() {
            return Err(Error("server host is empty".into()));
        }
        Ok((host.to_owned(), port))
    } else {
        Ok((authority.to_owned(), default_port))
    }
}

#[cfg(test)]
mod tests {
    use super::{ParsedServer, parse_authority};

    #[test]
    fn parses_https_server() {
        let server = ParsedServer::parse("https://ntfy.sh/").unwrap();
        assert!(server.secure);
        assert_eq!(server.host, "ntfy.sh");
        assert_eq!(server.port, 443);
        assert!(server.base_path.is_empty());
    }

    #[test]
    fn parses_self_hosted_path_and_port() {
        let server = ParsedServer::parse("http://localhost:8080/ntfy").unwrap();
        assert!(!server.secure);
        assert_eq!(server.port, 8080);
        assert_eq!(server.base_path, "/ntfy");
    }

    #[test]
    fn parses_ipv6_authority() {
        assert_eq!(
            parse_authority("[::1]:8080", 80).unwrap(),
            ("::1".into(), 8080)
        );
    }
}
