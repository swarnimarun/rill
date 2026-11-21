use super::{worker, Site};
use serde_json::{json, Value};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
};

const JSON: &str = "application/json; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";
const HEAD_LIMIT: usize = 16 * 1024;

struct Request {
    method: String,
    path: String,
    fields: BTreeMap<String, String>,
    length: usize,
}

fn line(input: &mut impl BufRead, remaining: &mut usize) -> Result<String, &'static str> {
    let mut bytes = Vec::new();
    let n = input
        .take(*remaining as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "could not read HTTP headers")?;
    *remaining -= n;
    let data = bytes
        .strip_suffix(b"\r\n")
        .ok_or("incomplete or oversized HTTP headers")?;
    String::from_utf8(data.to_vec()).map_err(|_| "invalid HTTP header encoding")
}

fn request(input: &mut impl BufRead, port: u16) -> Result<Request, &'static str> {
    let mut budget = HEAD_LIMIT;
    let first = line(input, &mut budget)?;
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or_default();
    let path = parts.next().ok_or("missing request path")?;
    if !matches!(parts.next(), Some("HTTP/1.0" | "HTTP/1.1"))
        || parts.next().is_some()
        || !path.starts_with('/')
    {
        return Err("invalid HTTP request line");
    }
    let mut fields = BTreeMap::new();
    loop {
        let header = line(input, &mut budget)?;
        if header.is_empty() {
            break;
        }
        let (key, value) = header.split_once(':').ok_or("invalid HTTP header")?;
        if key.is_empty() || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return Err("invalid HTTP header name");
        }
        let key = key.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "content-length" | "content-type" | "host" | "origin" | "transfer-encoding"
        ) && fields.insert(key, value.trim().to_owned()).is_some()
        {
            return Err("duplicate HTTP header");
        }
    }
    let host = fields.get("host").ok_or("missing local host")?;
    if !["127.0.0.1", "localhost"]
        .iter()
        .any(|h| host == &format!("{h}:{port}") || (port == 80 && host == h))
    {
        return Err("expected the local playground host");
    }
    if fields
        .get("origin")
        .is_some_and(|origin| origin != &format!("http://{host}"))
    {
        return Err("expected a same-origin request");
    }
    if fields.contains_key("transfer-encoding") {
        return Err("use a content-length framed request");
    }
    let length = match fields.get("content-length") {
        Some(n) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => {
            n.parse().map_err(|_| "invalid content length")?
        }
        Some(_) => return Err("invalid content length"),
        None => 0,
    };
    Ok(Request {
        method: method.into(),
        path: path.split('?').next().unwrap().to_owned(),
        fields,
        length,
    })
}

struct Response<'a> {
    status: &'static str,
    mime: &'static str,
    data: Cow<'a, [u8]>,
}

impl<'a> Response<'a> {
    fn page(mime: &'static str, bytes: &'a [u8]) -> Self {
        Self {
            status: "200 OK",
            mime,
            data: bytes.into(),
        }
    }
    fn error(status: &'static str, diagnostics: &str) -> Self {
        Self {
            status,
            mime: JSON,
            data: json!({"ok":false, "output":"", "diagnostics":diagnostics})
                .to_string()
                .into_bytes()
                .into(),
        }
    }
    fn write(&self, stream: &mut impl Write, head: bool) -> io::Result<()> {
        write!(stream,"HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'\r\n",self.status,self.mime,self.data.len())?;
        if self.status == "308 Permanent Redirect" {
            stream.write_all(b"Location: /book/\r\n")?;
        }
        stream.write_all(b"\r\n")?;
        if !head {
            stream.write_all(&self.data)?;
        }
        stream.flush()
    }
}

fn static_file<'a>(site: &'a Site, path: &str) -> Response<'a> {
    match path {
        "/" => Response::page(HTML, include_bytes!("assets/index.html")),
        "/assets/style.css" => Response::page("text/css", include_bytes!("assets/style.css")),
        "/assets/app.js" => Response::page("text/javascript", include_bytes!("assets/app.js")),
        "/api/lessons" => Response::page(JSON, site.lessons.as_bytes()),
        "/book" => Response {
            status: "308 Permanent Redirect",
            mime: HTML,
            data: Cow::Borrowed(b""),
        },
        path => {
            if let Some(key) = path.strip_prefix("/book/") {
                let key = if key.is_empty() { "index.html" } else { key };
                if let Some(bytes) = site.book.get(key) {
                    let mime = if key.ends_with(".html") {
                        HTML
                    } else if key.ends_with(".css") {
                        "text/css"
                    } else if key.ends_with(".js") {
                        "text/javascript"
                    } else {
                        "application/octet-stream"
                    };
                    return Response::page(mime, bytes);
                }
            }
            Response::error("404 Not Found", "unknown playground resource")
        }
    }
}

fn respond<'a>(site: &'a Site, req: &Request, reader: &mut impl Read) -> Response<'a> {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET" | "HEAD", _) if req.length == 0 => static_file(site, &req.path),
        ("POST", "/api/inspect") => {
            if req.length > worker::MAX_REQUEST {
                return Response::error("413 Content Too Large", "playground request too large");
            }
            if !req
                .fields
                .get("content-type")
                .and_then(|s| s.split(';').next())
                .is_some_and(|s| s.trim().eq_ignore_ascii_case("application/json"))
            {
                return Response::error("415 Unsupported Media Type", "expected application/json");
            }
            let mut body = vec![0; req.length];
            if reader.read_exact(&mut body).is_err()
                || serde_json::from_slice::<Value>(&body).is_err()
            {
                return Response::error("400 Bad Request", "expected a complete JSON request");
            }
            match worker::execute(&body) {
                Ok(data) => Response {
                    status: "200 OK",
                    mime: JSON,
                    data: data.into(),
                },
                Err(message) => Response::error("422 Unprocessable Content", &message),
            }
        }
        _ => Response::error(
            "405 Method Not Allowed",
            "use GET for pages and POST for compiler requests",
        ),
    }
}

pub(super) fn serve(site: &Site, stream: &mut TcpStream) -> io::Result<()> {
    let mut reader = BufReader::new(&*stream);
    let (response, head) = match request(&mut reader, site.port) {
        Ok(req) => (respond(site, &req, &mut reader), req.method == "HEAD"),
        Err(message) => (Response::error("400 Bad Request", message), false),
    };
    response.write(stream, head)
}

pub(super) fn busy(stream: &mut TcpStream) -> io::Result<()> {
    Response::error("503 Service Unavailable", "playground busy, try again").write(stream, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_local_origin_can_submit_a_bounded_request() {
        let parse = |s: &str| request(&mut s.as_bytes(), 7878);
        let valid = "POST /api/inspect HTTP/1.1\r\nHost: 127.0.0.1:7878\r\nOrigin: http://127.0.0.1:7878\r\nContent-Length: 12\r\n\r\n";
        assert_eq!(parse(valid).unwrap().length, 12);
        for bad in [
            valid.replace("Host: 127.0.0.1:7878", "Host: example.org"),
            valid.replace(
                "Origin: http://127.0.0.1:7878",
                "Origin: http://example.org",
            ),
            valid.replace(
                "Content-Length: 12",
                "Content-Length: 12\r\nContent-Length: 20",
            ),
            valid.replace("Content-Length: 12", "Transfer-Encoding: chunked"),
            valid.replace("Content-Length: 12", &"x".repeat(HEAD_LIMIT)),
        ] {
            assert!(parse(&bad).is_err());
        }
    }
}
