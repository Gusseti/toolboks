//! Minimal HTTPS GET on top of WinHTTP: no extra TLS library in the exe, and it follows the
//! system proxy settings. Responses are returned in memory; nothing touches the disk.

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
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

/// GET `url` and return the body. Redirects are followed by WinHTTP.
/// Fails with `Other` if the body would exceed `max_bytes`.
pub fn get(url: &str, accept: &str, max_bytes: usize) -> Result<Vec<u8>, HttpError> {
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

        let mut body = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as u32,
                &mut read,
            )
            .map_err(|e| other("WinHttpReadData", e))?;
            if read == 0 {
                return Ok(body);
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
        let body = get(
            "https://api.github.com/repos/rust-lang/rust/releases/latest",
            "application/vnd.github+json",
            1024 * 1024,
        )
        .expect("GET failed");
        assert!(String::from_utf8_lossy(&body).contains("tag_name"));
    }

    /// Needs internet; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn reports_404_as_status() {
        let err = get(
            "https://api.github.com/repos/rust-lang/rust/releases/does-not-exist",
            "application/vnd.github+json",
            1024 * 1024,
        )
        .unwrap_err();
        assert_eq!(err, HttpError::Status(404));
    }
}
