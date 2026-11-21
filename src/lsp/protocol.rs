use serde_json::Value;
use std::io::{self, BufRead, Read, Write};

const MAX_HEADER: usize = 8192;
const MAX_FRAME: usize = 8 * 1024 * 1024;

pub(super) fn read(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let (mut total, mut length) = (0, None);
    loop {
        let mut line = Vec::new();
        let count = (&mut *reader)
            .take((MAX_HEADER - total + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 && total == 0 {
            return Ok(None);
        }
        total += count;
        if total > MAX_HEADER || !line.ends_with(b"\r\n") || !line.is_ascii() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid LSP header",
            ));
        }
        if line == b"\r\n" {
            break;
        }
        let line = std::str::from_utf8(&line[..line.len() - 2]).unwrap();
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed LSP header"))?;
        if name.eq_ignore_ascii_case("Content-Length") {
            let value = value.trim();
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid Content-Length",
                ));
            }
            length = Some(value.parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
            })?);
        }
    }
    let length = length.filter(|n| *n <= MAX_FRAME).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "missing or oversized Content-Length",
        )
    })?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub(super) fn write(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_count_utf8_bytes_and_keep_message_boundaries() {
        let message = serde_json::json!({"text":"é🦀"});
        let mut bytes = Vec::new();
        write(&mut bytes, &message).unwrap();
        write(&mut bytes, &message).unwrap();
        let mut input = Cursor::new(bytes);
        for _ in 0..2 {
            let body = read(&mut input).unwrap().unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), message);
        }
        assert!(read(&mut input).unwrap().is_none());
    }

    #[test]
    fn invalid_and_unbounded_frames_fail_before_body_allocation() {
        for header in [
            "Content-Length: 1\r\nContent-Length: 1\r\n\r\nx",
            "Content-Length: -1\r\n\r\n",
            "Content-Length: 99999999999999999999\r\n\r\n",
            "Content-Length: 8388609\r\n\r\n",
            "Content-Length: 1\n\nx",
            "Content-Type: utf8\r\n\r\n",
            "Content-Length: 2\r\n\r\nx",
        ] {
            assert!(read(&mut Cursor::new(header)).is_err(), "{header:?}");
        }
        assert!(read(&mut Cursor::new(vec![b'x'; MAX_HEADER + 10])).is_err());
    }
}
