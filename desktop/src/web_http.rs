//! Small shared HTTP transport helpers. Never called from the audio callback.
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Read headers in chunks, leaving the body in the socket. Previously each byte
/// required a separate OS read, including on Windows and encrypted tailnet links.
pub fn headers(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut raw = Vec::with_capacity(1024);
    let mut chunk = [0; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Header timeout"));
        }
        stream.set_read_timeout(Some(remaining))?;
        let available = stream.peek(&mut chunk)?;
        if available == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Client closed headers",
            ));
        }
        // Check the boundary between the preceding chunk and this one as well.
        let previous = raw.len();
        raw.extend_from_slice(&chunk[..available]);
        if let Some(end) = raw.windows(4).position(|v| v == b"\r\n\r\n").map(|n| n + 4) {
            if end > 8192 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Headers too large",
                ));
            }
            stream.read_exact(&mut chunk[..end - previous])?;
            raw.truncate(end);
            stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
            return Ok(raw);
        }
        if raw.len() >= 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Headers too large",
            ));
        }
        stream.read_exact(&mut chunk[..available])?;
    }
}

pub fn bind_error(service: &str, host: &str, port: u16, error: io::Error) -> String {
    format!(
        "No se pudo abrir {service} en {host}:{port}. Comprueba que otra instancia no esté usando este puerto o elige otro. Detalle: {error}"
    )
}

/// Return an HTTP error when possible instead of a silent socket close (fetch's
/// generic network error). Never log cookies, passwords or personal link keys.
pub fn request_error(stream: &mut TcpStream, error: io::Error) -> io::Result<()> {
    let status = match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => 408,
        io::ErrorKind::InvalidData => 413,
        io::ErrorKind::UnexpectedEof => return Ok(()), // Browser preconnect/cancel.
        _ => 400,
    };
    let body = if status == 408 {
        r#"{"error":"Solicitud incompleta o lenta; vuelve a conectar"}"#
    } else {
        r#"{"error":"Solicitud HTTP no válida"}"#
    };
    write!(
        stream,
        "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};
    #[test]
    fn fragmented_headers_preserve_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let head = headers(&mut stream).unwrap();
            assert!(head.ends_with(b"\r\n\r\n"));
            let mut body = [0; 4];
            stream.read_exact(&mut body).unwrap();
            assert_eq!(&body, b"test");
        });
        let mut client = TcpStream::connect(address).unwrap();
        client
            .write_all(b"POST /mix HTTP/1.1\r\nContent-Length: 4\r\n\r")
            .unwrap();
        thread::sleep(Duration::from_millis(10));
        client.write_all(b"\ntest").unwrap();
        worker.join().unwrap();
    }
}
