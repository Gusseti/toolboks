//! Minimal HTTPS GET on top of WinHTTP: no extra TLS library in the exe, and it follows the
//! system proxy settings. [`get`] returns small responses in memory; [`download`] streams a body
//! into a file with progress, cancel and resume.

use std::ffi::c_void;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpQueryOption, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_URL, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

const HTTPS_PORT: u16 = 443;
const TIMEOUT_MS: i32 = 15_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// The server answered with a non-2xx status.
    Status(u32),
    /// The request could not be made or completed.
    Other(String),
    /// The caller cancelled a streamed download.
    Cancelled,
}

/// Closes a WinHTTP handle on every exit path.
struct Internet(*mut c_void);

impl Internet {
    fn new(handle: *mut c_void, what: &str) -> Result<Self, HttpError> {
        if handle.is_null() {
            Err(HttpError::Other(format!(
                "{what} failed: {}",
                windows::core::Error::from_thread()
            )))
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for Internet {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful WinHTTP call and is closed exactly once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Splits `https://host/path?query` into host and path. Only HTTPS is accepted.
fn split_https_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    (!host.is_empty() && !host.contains('@') && !host.contains(':')).then_some((host, path))
}

fn other(what: &str, e: windows::core::Error) -> HttpError {
    HttpError::Other(format!("{what} failed: {e}"))
}

/// A finished download and the URL it really came from after any redirects.
#[derive(Debug)]
pub struct Response {
    pub body: Vec<u8>,
    pub final_url: String,
}

/// Where a request goes. Release code only ever builds this through [`Endpoint::https`], so it is
/// always HTTPS on port 443 through the system proxy. Plain HTTP to 127.0.0.1 exists for tests.
struct Endpoint<'a> {
    host: &'a str,
    path: &'a str,
    port: u16,
    secure: bool,
}

impl<'a> Endpoint<'a> {
    fn https(url: &'a str) -> Result<Self, HttpError> {
        let (host, path) = split_https_url(url)
            .ok_or_else(|| HttpError::Other("only plain https:// URLs are supported".into()))?;
        Ok(Self {
            host,
            path,
            port: HTTPS_PORT,
            secure: true,
        })
    }
}

/// An open request with a 2xx answer. Fields drop in order: request, connection, session.
struct Open {
    request: Internet,
    _connection: Internet,
    _session: Internet,
    status: u32,
    final_url: String,
}

/// Sends a GET with the given extra header lines and reads the status and the final URL.
fn open(endpoint: &Endpoint, headers: &str) -> Result<Open, HttpError> {
    let (host_w, path_w) = (wide(endpoint.host), wide(endpoint.path));
    // A loopback test server must not be reached through a proxy.
    let (access, flags) = if endpoint.secure {
        (WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE)
    } else {
        (WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_OPEN_REQUEST_FLAGS(0))
    };

    // SAFETY (the whole block): every pointer passed below is valid for the call, strings are
    // NUL-terminated, and each handle is wrapped in `Internet` right after it is created.
    unsafe {
        let session = Internet::new(
            WinHttpOpen(w!("toolboks"), access, PCWSTR::null(), PCWSTR::null(), 0),
            "WinHttpOpen",
        )?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS)
            .map_err(|e| other("WinHttpSetTimeouts", e))?;

        let connection = Internet::new(
            WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), endpoint.port, 0),
            "WinHttpConnect",
        )?;
        let request = Internet::new(
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            ),
            "WinHttpOpenRequest",
        )?;

        let headers: Vec<u16> = headers.encode_utf16().collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)
            .map_err(|e| other("WinHttpSendRequest", e))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|e| other("WinHttpReceiveResponse", e))?;

        let mut status = 0u32;
        let mut len = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(|e| other("WinHttpQueryHeaders", e))?;
        if !(200..300).contains(&status) {
            return Err(HttpError::Status(status));
        }

        // Where the request ended up after redirects; fail closed if it cannot be read.
        let mut url_buf = [0u16; 2048];
        let mut url_bytes = size_of_val(&url_buf) as u32;
        WinHttpQueryOption(
            request.0,
            WINHTTP_OPTION_URL,
            Some(url_buf.as_mut_ptr() as *mut c_void),
            &mut url_bytes,
        )
        .map_err(|e| other("WinHttpQueryOption", e))?;
        let final_url = String::from_utf16_lossy(&url_buf[..url_bytes as usize / 2])
            .trim_end_matches('\0')
            .to_string();

        Ok(Open {
            request,
            _connection: connection,
            _session: session,
            status,
            final_url,
        })
    }
}

/// Reads the next chunk into `chunk`; 0 means the body is complete.
fn read_chunk(open: &Open, chunk: &mut [u8]) -> Result<usize, HttpError> {
    let mut read = 0u32;
    // SAFETY: `chunk` is valid for `chunk.len()` bytes and the request handle is open.
    unsafe {
        WinHttpReadData(
            open.request.0,
            chunk.as_mut_ptr() as *mut c_void,
            chunk.len() as u32,
            &mut read,
        )
    }
    .map_err(|e| other("WinHttpReadData", e))?;
    Ok(read as usize)
}

/// GET `url` and return the body. Redirects are followed by WinHTTP, so check `final_url`.
/// Fails with `Other` if the body would exceed `max_bytes` or the whole transfer takes longer
/// than `deadline` (plus at most one read timeout).
pub fn get(
    url: &str,
    accept: &str,
    max_bytes: usize,
    deadline: Duration,
) -> Result<Response, HttpError> {
    let started = Instant::now();
    let open = open(&Endpoint::https(url)?, &format!("Accept: {accept}\r\n"))?;
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if started.elapsed() > deadline {
            return Err(HttpError::Other("the download took too long".into()));
        }
        let read = read_chunk(&open, &mut chunk)?;
        if read == 0 {
            return Ok(Response {
                body,
                final_url: open.final_url,
            });
        }
        if body.len() + read > max_bytes {
            return Err(HttpError::Other(format!(
                "response is larger than {max_bytes} bytes"
            )));
        }
        body.extend_from_slice(&chunk[..read]);
    }
}

/// How a streamed download runs.
pub struct Download<'a> {
    /// Bytes already in the file from an earlier, interrupted attempt. When non-zero a `Range`
    /// request is sent; a server that ignores it (200) makes the file start over.
    pub resume_from: u64,
    /// The file may never grow past this.
    pub max_bytes: u64,
    pub deadline: Duration,
    /// Checked before every read; set it to stop. The partial file stays for a later resume.
    pub cancel: &'a AtomicBool,
    /// Called with the number of bytes in the file after every chunk.
    pub progress: &'a mut dyn FnMut(u64),
}

/// What a streamed download did.
#[derive(Debug, PartialEq, Eq)]
pub struct Saved {
    pub final_url: String,
    /// The server honoured the `Range` request and the bytes were appended.
    pub resumed: bool,
    /// Bytes in the file now.
    pub len: u64,
}

/// GET `url` (HTTPS on port 443 only) and stream the body into `file`, with progress, cancel and
/// resume. Redirects are followed by WinHTTP, so check `final_url`. The caller verifies the bytes.
pub fn download(url: &str, file: &Path, options: Download) -> Result<Saved, HttpError> {
    download_from(&Endpoint::https(url)?, file, options)
}

fn download_from(endpoint: &Endpoint, file: &Path, options: Download) -> Result<Saved, HttpError> {
    let started = Instant::now();
    let io = |e: std::io::Error| HttpError::Other(format!("could not write the download: {e}"));
    let mut headers = String::from("Accept: application/octet-stream\r\n");
    if options.resume_from > 0 {
        headers.push_str(&format!("Range: bytes={}-\r\n", options.resume_from));
    }
    let open = open(endpoint, &headers)?;
    let resumed = options.resume_from > 0 && open.status == 206;
    if open.status == 206 && !resumed {
        return Err(HttpError::Other("unexpected partial response".into()));
    }
    let mut out = if resumed {
        let out = std::fs::OpenOptions::new()
            .append(true)
            .open(file)
            .map_err(io)?;
        if out.metadata().map_err(io)?.len() != options.resume_from {
            return Err(HttpError::Other("the partial file changed".into()));
        }
        out
    } else {
        std::fs::File::create(file).map_err(io)?
    };
    let mut len = if resumed { options.resume_from } else { 0 };
    (options.progress)(len);
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if options.cancel.load(Ordering::Relaxed) {
            return Err(HttpError::Cancelled);
        }
        if started.elapsed() > options.deadline {
            return Err(HttpError::Other("the download took too long".into()));
        }
        let read = read_chunk(&open, &mut chunk)?;
        if read == 0 {
            out.sync_all().map_err(io)?;
            return Ok(Saved {
                final_url: open.final_url,
                resumed,
                len,
            });
        }
        if len + read as u64 > options.max_bytes {
            return Err(HttpError::Other(format!(
                "response is larger than {} bytes",
                options.max_bytes
            )));
        }
        out.write_all(&chunk[..read]).map_err(io)?;
        len += read as u64;
        (options.progress)(len);
    }
}

/// Test hook: the same streamed download against a plain-HTTP server on `127.0.0.1` with any
/// port. It is compiled into test builds only, so no release code path can reach it.
#[cfg(test)]
pub fn download_local(url: &str, file: &Path, options: Download) -> Result<Saved, HttpError> {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| HttpError::Other("test URLs are http://127.0.0.1:<port>/...".into()))?;
    let (port, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let port = port
        .parse()
        .map_err(|_| HttpError::Other("bad port".into()))?;
    let endpoint = Endpoint {
        host: "127.0.0.1",
        path: if path.is_empty() { "/" } else { path },
        port,
        secure: false,
    };
    download_from(&endpoint, file, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_urls() {
        assert_eq!(
            split_https_url("https://api.github.com/repos/a/b?x=1"),
            Some(("api.github.com", "/repos/a/b?x=1"))
        );
        assert_eq!(
            split_https_url("https://example.com"),
            Some(("example.com", "/"))
        );
    }

    #[test]
    fn rejects_non_https_and_odd_hosts() {
        assert_eq!(split_https_url("http://example.com/x"), None);
        assert_eq!(split_https_url("https:///x"), None);
        assert_eq!(split_https_url("https://user@example.com/x"), None);
        assert_eq!(split_https_url("https://example.com:8443/x"), None);
    }

    #[test]
    fn release_download_refuses_plain_http_and_custom_ports() {
        let cancel = AtomicBool::new(false);
        let file = std::env::temp_dir().join(format!("toolboks-http-{}", std::process::id()));
        for url in ["http://127.0.0.1:8080/x", "https://127.0.0.1:8443/x"] {
            let result = download(
                url,
                &file,
                Download {
                    resume_from: 0,
                    max_bytes: 10,
                    deadline: Duration::from_secs(1),
                    cancel: &cancel,
                    progress: &mut |_| {},
                },
            );
            assert!(matches!(result, Err(HttpError::Other(_))), "{url}");
        }
        assert!(!file.exists());
    }

    /// Needs internet; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_a_public_url() {
        let response = get(
            "https://api.github.com/repos/rust-lang/rust/releases/latest",
            "application/vnd.github+json",
            1024 * 1024,
            Duration::from_secs(30),
        )
        .expect("GET failed");
        assert!(String::from_utf8_lossy(&response.body).contains("tag_name"));
        assert!(response.final_url.starts_with("https://api.github.com/"));
    }

    /// Needs internet; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn reports_the_url_after_a_redirect() {
        let response = get(
            "https://github.com/rust-lang/rust/releases/latest",
            "text/html",
            16 * 1024 * 1024,
            Duration::from_secs(30),
        )
        .expect("GET failed");
        assert!(
            response.final_url.contains("/releases/tag/"),
            "{}",
            response.final_url
        );
    }

    #[test]
    fn a_zero_deadline_aborts_the_transfer() {
        // The deadline is checked before every read, so this must fail without hanging.
        let result = get(
            "https://api.github.com/repos/rust-lang/rust/releases/latest",
            "application/vnd.github+json",
            1024 * 1024,
            Duration::ZERO,
        );
        // Offline machines fail earlier with a network error; both are errors.
        assert!(result.is_err());
    }

    /// Needs internet; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn reports_404_as_status() {
        let err = get(
            "https://api.github.com/repos/rust-lang/rust/releases/does-not-exist",
            "application/vnd.github+json",
            1024 * 1024,
            Duration::from_secs(30),
        )
        .unwrap_err();
        assert_eq!(err, HttpError::Status(404));
    }
}
