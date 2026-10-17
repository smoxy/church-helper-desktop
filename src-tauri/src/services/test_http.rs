//! Minimal one-shot HTTP server for unit tests (no extra dev-dependency):
//! binds 127.0.0.1 on an ephemeral port, answers the FIRST request with the
//! given status/body, then closes. Used to exercise real HTTP status handling
//! (download status check, best-effort next-week fetch) end to end.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Start the server and return its base URL (`http://127.0.0.1:<port>`).
/// `status` is the status line tail, e.g. `"404 Not Found"`.
pub(crate) async fn serve_once(status: &str, content_type: &str, body: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("listener addr");
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            // Read (and discard) the request head before answering.
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    format!("http://{addr}")
}
