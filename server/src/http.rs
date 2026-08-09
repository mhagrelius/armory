//! Enough HTTP to serve the routes an account needs.
//!
//! Request line, headers to the blank line, a body whose length the headers
//! gave, one response, close. No keep-alive, no chunked encoding, no TLS —
//! this listens on a tailnet address and speaks to one person's machines, and
//! WireGuard is doing the encrypting. If any of those stops being true this
//! should take a real server rather than grow into one.
//!
//! The parsing is the part worth testing, so it is a function over bytes
//! rather than something tangled up with a socket.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

/// A parsed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    /// A header, matched without regard to case as the spec requires.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Which installation is calling.
    ///
    /// Not authentication — every machine shares the one token. It is what
    /// the log is stamped with and what a pull is filtered by, so that a
    /// machine is never handed back the rows it just sent.
    pub fn machine(&self) -> &str {
        self.header("x-armory-machine").unwrap_or_default()
    }

    /// A query-string value, percent-decoded.
    ///
    /// Forty lines of parsing rather than a crate, for three parameters that
    /// are all integers today. `+` is a space here because that is what a
    /// query string means by it, even though nothing this serves sends one.
    pub fn query(&self, name: &str) -> Option<String> {
        let (_, query) = self.path.split_once('?')?;
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (decode(key) == name).then(|| decode(value))
        })
    }

    /// The path with any query string taken off.
    pub fn route(&self) -> &str {
        self.path.split('?').next().unwrap_or(&self.path)
    }

    /// The bearer token, if one was offered.
    pub fn bearer(&self) -> Option<&str> {
        self.header("authorization")?
            .strip_prefix("Bearer ")
            .map(str::trim)
    }
}

fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&raw[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    // Not an escape after all. A literal `%` is likelier than
                    // a request worth refusing over.
                    Err(_) => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Why a request could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadRequest(pub String);

/// The largest body this will read.
///
/// Larger than a task list's server needs, because the largest thing here is
/// not a batch of small records: a cached API body travels base64-encoded and
/// `sync::MAX_BODY` lets one through at four megabytes, which is five and a
/// half encoded. A batch carries several. This is sized to hold a full one
/// with room over, and exists so that a malformed `Content-Length` cannot ask
/// the server to allocate a gigabyte.
pub const MAX_BODY: usize = 64 * 1024 * 1024;

/// Read one request from a stream.
pub fn read_request<R: Read>(stream: R) -> Result<Request, BadRequest> {
    let mut reader = BufReader::new(stream);

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| BadRequest(error.to_string()))?;

    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| BadRequest("empty request line".into()))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| BadRequest("no path".into()))?
        .to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| BadRequest(error.to_string()))?;
        // End of stream before the blank line: the client hung up mid-request.
        if read == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let length: usize = match headers.get("content-length") {
        Some(value) => value
            .parse()
            .map_err(|_| BadRequest(format!("content-length is not a number: {value}")))?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err(BadRequest(format!("body of {length} bytes is too large")));
    }

    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|error| BadRequest(error.to_string()))?;

    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

/// What to send back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

impl Response {
    pub fn json(status: u16, value: &impl serde::Serialize) -> Self {
        // A response that will not serialise is a bug here rather than
        // anything the client did, and saying so beats sending half of one.
        let body = serde_json::to_vec(value)
            .unwrap_or_else(|error| format!(r#"{{"error":"{error}"}}"#).into_bytes());
        Self {
            status,
            body,
            content_type: "application/json",
        }
    }

    pub fn text(status: u16, message: &str) -> Self {
        Self {
            status,
            body: message.as_bytes().to_vec(),
            content_type: "text/plain; charset=utf-8",
        }
    }
}

/// Write a response and finish with the connection.
pub fn write_response<W: Write>(mut stream: W, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };

    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    )?;
    stream.write_all(&response.body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(raw: &str) -> Result<Request, BadRequest> {
        read_request(raw.as_bytes())
    }

    #[test]
    fn a_plain_get_parses() {
        let parsed = request("GET /health HTTP/1.1\r\nHost: nas\r\n\r\n").expect("a request");
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.path, "/health");
        assert_eq!(parsed.header("host"), Some("nas"));
        assert!(parsed.body.is_empty());
    }

    #[test]
    fn a_body_is_read_to_the_length_the_headers_gave() {
        let parsed = request("POST /records HTTP/1.1\r\nContent-Length: 7\r\n\r\n{\"a\":1}")
            .expect("a request");
        assert_eq!(parsed.body, b"{\"a\":1}");
    }

    #[test]
    fn header_names_match_whatever_case_they_arrive_in() {
        let parsed =
            request("GET / HTTP/1.1\r\nCONTENT-length: 0\r\nAuthorization: Bearer hunter2\r\n\r\n")
                .expect("a request");
        assert_eq!(parsed.header("content-length"), Some("0"));
        assert_eq!(parsed.bearer(), Some("hunter2"));
    }

    #[test]
    fn no_authorization_header_is_no_token_rather_than_an_error() {
        let parsed = request("GET / HTTP/1.1\r\n\r\n").expect("a request");
        assert_eq!(parsed.bearer(), None);
    }

    #[test]
    fn a_content_length_that_is_not_a_number_is_refused() {
        let error = request("POST / HTTP/1.1\r\nContent-Length: lots\r\n\r\n")
            .expect_err("that is not a length");
        assert!(error.0.contains("lots"), "{}", error.0);
    }

    #[test]
    fn an_absurd_content_length_is_refused_before_anything_is_allocated() {
        let raw = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        let error = request(&raw).expect_err("too large");
        assert!(error.0.contains("too large"), "{}", error.0);
    }

    #[test]
    fn a_body_shorter_than_its_length_is_refused_rather_than_padded() {
        // Otherwise a client that hangs up mid-write would look like one that
        // sent an empty record.
        let error =
            request("POST / HTTP/1.1\r\nContent-Length: 40\r\n\r\nshort").expect_err("truncated");
        assert!(!error.0.is_empty());
    }

    #[test]
    fn a_query_string_is_read_off_the_path_and_the_route_is_what_is_left() {
        let parsed = request("GET /pull?since=42&limit=500 HTTP/1.1\r\n\r\n").expect("a request");
        assert_eq!(parsed.route(), "/pull");
        assert_eq!(parsed.query("since").as_deref(), Some("42"));
        assert_eq!(parsed.query("limit").as_deref(), Some("500"));
        assert_eq!(parsed.query("machine"), None);
    }

    #[test]
    fn a_path_with_no_query_is_all_route() {
        let parsed = request("GET /health HTTP/1.1\r\n\r\n").expect("a request");
        assert_eq!(parsed.route(), "/health");
        assert_eq!(parsed.query("since"), None);
    }

    #[test]
    fn a_percent_escape_is_decoded_and_a_stray_percent_is_not_fatal() {
        let parsed = request("GET /pull?machine=a%2Db%20c&other=100%25 HTTP/1.1\r\n\r\n")
            .expect("a request");
        assert_eq!(parsed.query("machine").as_deref(), Some("a-b c"));
        assert_eq!(parsed.query("other").as_deref(), Some("100%"));
    }

    #[test]
    fn the_calling_machine_is_a_header_and_absent_is_empty_rather_than_an_error() {
        let named =
            request("GET / HTTP/1.1\r\nX-Armory-Machine: kitchen\r\n\r\n").expect("a request");
        assert_eq!(named.machine(), "kitchen");
        assert_eq!(request("GET / HTTP/1.1\r\n\r\n").unwrap().machine(), "");
    }

    #[test]
    fn a_response_carries_its_own_length() {
        let mut written = Vec::new();
        write_response(&mut written, &Response::text(404, "no such route")).expect("write");
        let text = String::from_utf8(written).expect("utf-8");

        assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"), "{text}");
        assert!(text.contains("Content-Length: 13\r\n"), "{text}");
        assert!(text.ends_with("\r\n\r\nno such route"), "{text}");
    }
}
