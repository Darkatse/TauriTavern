use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(super) async fn upstream(
    bodies: Vec<String>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for body in bodies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let count = socket.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length: ")
                                .map(|value| value.parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            socket.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            if body.starts_with("data:") { "text/event-stream" } else { "application/json" },
            body.len(),
        ).as_bytes()).await.unwrap();
            requests.push(String::from_utf8(request).unwrap());
        }
        requests
    });
    (base, task)
}
