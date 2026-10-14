//! HTTP(S) requests through WinHTTP, Windows' own client (the user's proxy settings and
//! certificate store, no TLS library of Kabuk's): blocking, for a worker thread. Header
//! values are never logged, as they may hold an API key.

use std::ffi::c_void;
use std::time::Duration;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

/// What came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// A URL split for WinHTTP.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Parts {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

const HTTPS_PORT: u16 = 443;
const HTTP_PORT: u16 = 80;
const AGENT: &str = "Kabuk";
const READ_CHUNK: usize = 16 * 1024;

fn parts(url: &str) -> Option<Parts> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let (authority, path) = rest.find('/').map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, if secure { HTTPS_PORT } else { HTTP_PORT }),
    };
    (!host.is_empty()).then(|| Parts {
        secure,
        host: host.to_string(),
        port,
        path: path.to_string(),
    })
}

/// A WinHTTP handle, closed on drop.
struct Handle(*mut c_void);

impl Handle {
    fn new(h: *mut c_void) -> std::io::Result<Handle> {
        if h.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Handle(h))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP gave us, closed once.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn io(e: windows::core::Error) -> std::io::Error {
    std::io::Error::other(e)
}

fn ms(d: Duration) -> i32 {
    i32::try_from(d.as_millis()).unwrap_or(i32::MAX)
}

/// Sends a request: `POST` with `body`, or `GET` without. `timeout` bounds each of
/// resolving, connecting, sending and waiting for the answer.
pub fn send(
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
    timeout: Duration,
) -> std::io::Result<Response> {
    let p = parts(url).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "not an http(s) URL"))?;
    let verb = if body.is_some() { "POST" } else { "GET" };
    tracing::debug!(host = %p.host, path = %p.path, verb, "http request");
    // SAFETY: WinHTTP calls on handles opened here and closed by `Handle`'s drop, in
    // reverse order; every buffer outlives the call it is passed to.
    unsafe {
        let session = Handle::new(WinHttpOpen(
            &HSTRING::from(AGENT),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        let t = ms(timeout);
        WinHttpSetTimeouts(session.0, t, t, t, t).map_err(io)?;
        let connect = Handle::new(WinHttpConnect(session.0, &HSTRING::from(p.host.as_str()), p.port, 0))?;
        let flags = if p.secure {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let request = Handle::new(WinHttpOpenRequest(
            connect.0,
            &HSTRING::from(verb),
            &HSTRING::from(p.path.as_str()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        ))?;
        let lines: String = headers.iter().map(|(n, v)| format!("{n}: {v}\r\n")).collect();
        let mut wide: Vec<u16> = lines.encode_utf16().collect();
        if !wide.is_empty() {
            WinHttpAddRequestHeaders(request.0, &wide, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE)
                .map_err(io)?;
        }
        wide.iter_mut().for_each(|c| *c = 0);
        let data = body.unwrap_or("").as_bytes();
        let len = u32::try_from(data.len()).map_err(std::io::Error::other)?;
        WinHttpSendRequest(
            request.0,
            None,
            (!data.is_empty()).then_some(data.as_ptr().cast()),
            len,
            len,
            0,
        )
        .map_err(io)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(io)?;
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(io)?;
        let mut out = Vec::new();
        let mut chunk = vec![0u8; READ_CHUNK];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr().cast(),
                u32::try_from(chunk.len()).unwrap_or(u32::MAX),
                &mut read,
            )
            .map_err(io)?;
            if read == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..read as usize]);
        }
        Ok(Response {
            status: u16::try_from(status).unwrap_or(0),
            body: String::from_utf8_lossy(&out).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: URLs split with and without port and path, other schemes refused; a
    //! POST and a GET against a local server (the headers and body arrive, the status and
    //! body come back); nobody listening is an error, not a hang.
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    #[test]
    fn urls_are_split() {
        let p = parts("https://api.anthropic.com/v1/messages").unwrap();
        assert_eq!(
            (p.secure, p.host.as_str(), p.port, p.path.as_str()),
            (true, "api.anthropic.com", 443, "/v1/messages")
        );
        let l = parts("http://localhost:11434").unwrap();
        assert_eq!(
            (l.secure, l.host.as_str(), l.port, l.path.as_str()),
            (false, "localhost", 11434, "/")
        );
        assert_eq!(parts("ftp://x/"), None);
        assert_eq!(parts("http://:80/"), None);
        assert_eq!(parts("http://h:port/"), None);
    }

    /// Answers one request with `status` and `body`; returns what it received.
    fn serve(status: u16, body: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut b = vec![0u8; length];
            reader.read_exact(&mut b).unwrap();
            let mut s = stream;
            write!(
                s,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            head + &String::from_utf8(b).unwrap()
        });
        (port, h)
    }

    #[test]
    fn a_post_and_a_get_go_and_come_back() {
        let (port, got) = serve(200, "{\"ok\":true}");
        let headers = vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("x-test".to_string(), "1".to_string()),
        ];
        let r = send(
            &format!("http://127.0.0.1:{port}/api/chat"),
            &headers,
            Some("{\"a\":\"ğ\"}"),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(
            r,
            Response {
                status: 200,
                body: "{\"ok\":true}".into()
            }
        );
        let seen = got.join().unwrap();
        assert!(seen.starts_with("POST /api/chat HTTP/1.1"), "{seen}");
        assert!(seen.to_ascii_lowercase().contains("x-test: 1"));
        assert!(seen.ends_with("{\"a\":\"ğ\"}"));
        let (port, got) = serve(404, "nope");
        let r = send(
            &format!("http://127.0.0.1:{port}/api/tags"),
            &[],
            None,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!((r.status, r.body.as_str()), (404, "nope"));
        assert!(got.join().unwrap().starts_with("GET /api/tags"));
    }

    #[test]
    fn nobody_listening_is_an_error() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(send(&format!("http://127.0.0.1:{port}/"), &[], None, Duration::from_secs(2)).is_err());
    }
}
