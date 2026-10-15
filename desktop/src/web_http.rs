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
    // Winsock accept inherits the listener's nonblocking mode. HTTP workers
    // require blocking I/O with deadlines: WouldBlock must not become a false
    // 408 or truncate a large CSS/JS/JSON response. The WebSocket worker switches
    // back to nonblocking deliberately, after its HTTP upgrade completes.
    stream.set_nonblocking(false)?;
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
    fn inherited_nonblocking_socket_waits_for_headers_and_writes_complete_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Winsock accept inherits the nonblocking listener's mode; Linux
            // does not. Force that Windows precondition on every test host.
            stream.set_nonblocking(true).unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            ready_tx.send(()).unwrap();
            let head = headers(&mut stream).expect("HTTP headers must wait for incoming bytes");
            assert!(head.ends_with(b"\r\n\r\n"));
            let mut body = [0; 4];
            stream.read_exact(&mut body).unwrap();
            assert_eq!(&body, b"test");
            let response = vec![0x42; 8 * 1024 * 1024];
            stream
                .write_all(&response)
                .expect("HTTP response must survive send-buffer backpressure");
        });
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        ready_rx.recv().unwrap();
        thread::sleep(Duration::from_millis(30));
        let sent = client.write_all(b"POST /api/config HTTP/1.1\r\nContent-Length: 4\r\n");
        thread::sleep(Duration::from_millis(30));
        let completed = client.write_all(b"\r\ntest");
        thread::sleep(Duration::from_millis(30));
        let mut response = Vec::new();
        let read = client.read_to_end(&mut response);
        worker
            .join()
            .expect("HTTP worker failed with an inherited nonblocking socket");
        sent.unwrap();
        completed.unwrap();
        read.unwrap();
        assert_eq!(response.len(), 8 * 1024 * 1024);
        assert!(response.iter().all(|byte| *byte == 0x42));
    }

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
