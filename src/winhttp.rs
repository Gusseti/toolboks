//! Minimal HTTPS GET on top of WinHTTP: no extra TLS library in the exe, and it follows the
//! system proxy settings. Responses are returned in memory; nothing touches the disk.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpQueryOption, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPTION_URL, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

const HTTPS_PORT: u16 = 443;
const TIMEOUT_MS: i32 = 15_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// The server answered with a non-2xx status.
    Status(u32),
    /// The request could not be made or completed.
    Other(String),
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
    let (host, path) = split_https_url(url)
        .ok_or_else(|| HttpError::Other("only plain https:// URLs are supported".into()))?;
    let (host_w, path_w) = (wide(host), wide(path));

    // SAFETY (the whole block): every pointer passed below is valid for the call, strings are
    // NUL-terminated, and each handle is wrapped in `Internet` right after it is created.
    unsafe {
        let session = Internet::new(
            WinHttpOpen(
                w!("toolboks"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "WinHttpOpen",
        )?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS)
            .map_err(|e| other("WinHttpSetTimeouts", e))?;

        let connection = Internet::new(
            WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), HTTPS_PORT, 0),
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
                WINHTTP_FLAG_SECURE,
            ),
            "WinHttpOpenRequest",
        )?;

        let headers: Vec<u16> = format!("Accept: {accept}\r\n").encode_utf16().collect();
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

        let mut body = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if started.elapsed() > deadline {
                return Err(HttpError::Other("the download took too long".into()));
            }
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as u32,
                &mut read,
            )
            .map_err(|e| other("WinHttpReadData", e))?;
            if read == 0 {
                return Ok(Response { body, final_url });
            }
            if body.len() + read as usize > max_bytes {
                return Err(HttpError::Other(format!(
                    "response is larger than {max_bytes} bytes"
                )));
            }
            body.extend_from_slice(&chunk[..read as usize]);
        }
    }
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
