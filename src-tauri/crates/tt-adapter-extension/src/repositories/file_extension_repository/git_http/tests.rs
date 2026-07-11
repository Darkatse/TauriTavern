use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gix_transport::client::blocking_io::http::{Http, PostBodyDataKind};
use tt_adapter_http::HttpClientPool;
use uuid::Uuid;

use super::{GitHttp, shares_authority_or_upgrades_scheme};
use crate::repositories::file_extension_repository::git_remote::fetch_exact;

const TEST_USER_AGENT: &str = "TauriTavern/test";

#[derive(Clone, Debug)]
struct RecordedRequest {
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RecordedRequest {
    fn header_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.headers
            .iter()
            .filter(move |(key, _value)| key.eq_ignore_ascii_case(name))
            .map(|(_key, value)| value.as_str())
    }
}

struct TestServer {
    base_url: String,
    requests: Receiver<RecordedRequest>,
    handle: JoinHandle<io::Result<()>>,
}

impl TestServer {
    fn finish(self) {
        self.handle
            .join()
            .expect("server thread")
            .expect("server IO");
    }
}

fn spawn_server(
    request_count: usize,
    mut handler: impl FnMut(usize, &RecordedRequest, &mut TcpStream) -> io::Result<()> + Send + 'static,
) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let base_url = format!("http://{}", listener.local_addr().expect("server address"));
    let (request_tx, requests) = mpsc::channel();
    let handle = thread::spawn(move || {
        for index in 0..request_count {
            let (mut stream, _peer) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            let request = read_request(&stream)?;
            request_tx
                .send(request.clone())
                .map_err(|_| io::Error::other("request receiver dropped"))?;
            handler(index, &request, &mut stream)?;
        }
        Ok(())
    });

    TestServer {
        base_url,
        requests,
        handle,
    }
}

fn read_request(stream: &TcpStream) -> io::Result<RecordedRequest> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request target"))?
        .to_string();

    let mut headers = Vec::new();
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        let (name, value) = line
            .trim_end_matches(['\r', '\n'])
            .split_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid request header"))?;
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid content length")
            })?;
        }
        headers.push((name.to_string(), value));
    }

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(RecordedRequest {
        target,
        headers,
        body,
    })
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\n")?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(
        stream,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn ok(stream: &mut TcpStream, body: &[u8]) -> io::Result<()> {
    respond(stream, "200 OK", &[], body)
}

fn git_http() -> GitHttp {
    GitHttp::new(HttpClientPool::new(TEST_USER_AGENT).git_blocking_client_builder())
        .expect("build Git HTTP bridge")
}

#[test]
fn bounded_post_waits_for_writer_drop_and_executes_once() {
    let server = spawn_server(1, |_index, _request, stream| ok(stream, b"response"));
    let base = format!("{}/repo", server.base_url);
    let mut http = git_http();
    let response = http
        .post(
            &format!("{base}/git-upload-pack"),
            &base,
            ["Content-Type: application/x-git-upload-pack-request"],
            PostBodyDataKind::BoundedAndFitsIntoMemory,
        )
        .expect("prepare POST");
    let mut post_body = response.post_body;
    let mut headers = response.headers;
    let mut body = response.body;

    post_body.write_all(b"request").expect("write body");
    post_body.flush().expect("flush remains non-terminal");
    assert!(
        server
            .requests
            .recv_timeout(Duration::from_millis(50))
            .is_err()
    );

    drop(post_body);
    let mut header_text = String::new();
    headers
        .read_to_string(&mut header_text)
        .expect("read headers");
    let request = server
        .requests
        .recv_timeout(Duration::from_secs(1))
        .expect("POST request");
    assert_eq!(request.target, "/repo/git-upload-pack");
    assert_eq!(request.body, b"request");

    let mut response_body = String::new();
    body.read_to_string(&mut response_body)
        .expect("read response body");
    assert_eq!(response_body, "response");
    server.finish();
}

#[test]
fn reading_response_before_writer_drop_fails_without_network_io() {
    let mut http = git_http();
    let response = http
        .post(
            "http://127.0.0.1:9/repo/git-upload-pack",
            "http://127.0.0.1:9/repo",
            std::iter::empty::<&str>(),
            PostBodyDataKind::BoundedAndFitsIntoMemory,
        )
        .expect("prepare POST");
    let post_body = response.post_body;
    let mut headers = response.headers;

    let error = headers.fill_buf().expect_err("writer is still open");
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(
        error.to_string(),
        "Git HTTP response was read before the request body was closed"
    );
    drop(post_body);
}

#[test]
fn unbounded_post_is_rejected_immediately() {
    let mut http = git_http();
    let result = http.post(
        "http://127.0.0.1:9/repo/git-upload-pack",
        "http://127.0.0.1:9/repo",
        std::iter::empty::<&str>(),
        PostBodyDataKind::Unbounded,
    );
    let error = match result {
        Ok(_) => panic!("unbounded POST must fail"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("outside the supported transport contract")
    );
}

#[test]
fn request_headers_preserve_git_contract_and_replace_gix_user_agent() {
    let server = spawn_server(1, |_index, _request, stream| ok(stream, b"ok"));
    let base = format!("{}/repo", server.base_url);
    let mut http = git_http();
    let mut response = http
        .get(
            &format!("{base}/info/refs?service=git-upload-pack"),
            &base,
            [
                "User-Agent: git/oxide-0.57.2",
                "Git-Protocol: version=2",
                "Accept: application/x-git-upload-pack-advertisement",
                "X-Repeat: one",
                "X-Repeat: two",
            ],
        )
        .expect("prepare GET");
    response
        .headers
        .read_to_end(&mut Vec::new())
        .expect("send GET");

    let request = server
        .requests
        .recv_timeout(Duration::from_secs(1))
        .expect("GET request");
    assert_eq!(
        request.header_values("user-agent").collect::<Vec<_>>(),
        [TEST_USER_AGENT]
    );
    assert_eq!(
        request.header_values("git-protocol").collect::<Vec<_>>(),
        ["version=2"]
    );
    assert_eq!(
        request.header_values("x-repeat").collect::<Vec<_>>(),
        ["one", "two"]
    );
    server.finish();
}

#[test]
fn authenticated_headers_and_urls_are_rejected_without_secret_echo() {
    for header in [
        "Authorization: Bearer HEADER_SECRET",
        "Proxy-Authorization: Basic PROXY_SECRET",
    ] {
        let mut http = git_http();
        let result = http.get(
            "https://example.test/repo/info/refs?service=git-upload-pack",
            "https://example.test/repo",
            [header],
        );
        let error = match result {
            Ok(_) => panic!("authenticated header must fail"),
            Err(error) => error,
        };
        let text = error.to_string();
        assert!(text.contains("Authenticated Git HTTP requests are not supported"));
        assert!(!text.contains("SECRET"));
    }

    let mut http = git_http();
    let result = http.get(
        "https://user:URL_SECRET@example.test/repo/info/refs?service=git-upload-pack",
        "https://user:URL_SECRET@example.test/repo",
        std::iter::empty::<&str>(),
    );
    let error = match result {
        Ok(_) => panic!("authenticated URL must fail"),
        Err(error) => error,
    };
    assert!(!error.to_string().contains("URL_SECRET"));
}

#[test]
fn initial_redirect_updates_base_for_following_post() {
    let server = spawn_server(3, |index, _request, stream| match index {
        0 => respond(
            stream,
            "302 Found",
            &[("Location", "/new/info/refs?service=git-upload-pack")],
            b"",
        ),
        1 => ok(stream, b"advertisement"),
        2 => ok(stream, b"result"),
        _ => unreachable!(),
    });
    let original_base = format!("{}/old", server.base_url);
    let mut http = git_http();
    let mut advertisement = http
        .get(
            &format!("{original_base}/info/refs?service=git-upload-pack"),
            &original_base,
            std::iter::empty::<&str>(),
        )
        .expect("prepare advertisement");
    advertisement
        .headers
        .read_to_end(&mut Vec::new())
        .expect("follow initial redirect");
    let mut advertisement_body = String::new();
    advertisement
        .body
        .read_to_string(&mut advertisement_body)
        .expect("read advertisement");
    assert_eq!(advertisement_body, "advertisement");

    let rpc = http
        .post(
            &format!("{original_base}/git-upload-pack"),
            &original_base,
            std::iter::empty::<&str>(),
            PostBodyDataKind::BoundedAndFitsIntoMemory,
        )
        .expect("prepare RPC");
    let mut rpc_headers = rpc.headers;
    drop(rpc.post_body);
    rpc_headers.read_to_end(&mut Vec::new()).expect("send RPC");

    let targets = (0..3)
        .map(|_| {
            server
                .requests
                .recv_timeout(Duration::from_secs(1))
                .expect("redirect request")
                .target
        })
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        [
            "/old/info/refs?service=git-upload-pack",
            "/new/info/refs?service=git-upload-pack",
            "/new/git-upload-pack",
        ]
    );
    server.finish();
}

#[test]
fn redirect_authority_rules_reject_host_port_and_tls_downgrade() {
    let https = reqwest::Url::parse("https://example.test/repo").unwrap();
    let http = reqwest::Url::parse("http://example.test/repo").unwrap();

    assert!(shares_authority_or_upgrades_scheme(
        &reqwest::Url::parse("https://example.test/other").unwrap(),
        &https
    ));
    assert!(shares_authority_or_upgrades_scheme(
        &reqwest::Url::parse("https://example.test/other").unwrap(),
        &http
    ));
    assert!(!shares_authority_or_upgrades_scheme(
        &reqwest::Url::parse("http://example.test/other").unwrap(),
        &https
    ));
    assert!(!shares_authority_or_upgrades_scheme(
        &reqwest::Url::parse("https://other.test/repo").unwrap(),
        &https
    ));
    assert!(!shares_authority_or_upgrades_scheme(
        &reqwest::Url::parse("https://example.test:444/repo").unwrap(),
        &https
    ));
    assert!(!shares_authority_or_upgrades_scheme(
        &reqwest::Url::parse("https://user:secret@example.test/repo").unwrap(),
        &https
    ));
}

#[test]
fn post_redirect_is_not_followed() {
    let server = spawn_server(1, |_index, _request, stream| {
        respond(
            stream,
            "307 Temporary Redirect",
            &[("Location", "/other/git-upload-pack")],
            b"",
        )
    });
    let base = format!("{}/repo", server.base_url);
    let mut http = git_http();
    let response = http
        .post(
            &format!("{base}/git-upload-pack"),
            &base,
            std::iter::empty::<&str>(),
            PostBodyDataKind::BoundedAndFitsIntoMemory,
        )
        .expect("prepare POST");
    let mut headers = response.headers;
    drop(response.post_body);
    let error = headers.fill_buf().expect_err("POST redirect must fail");
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "Git HTTP request returned status 307");
    assert_eq!(
        server
            .requests
            .recv_timeout(Duration::from_secs(1))
            .expect("single POST")
            .target,
        "/repo/git-upload-pack"
    );
    server.finish();
}

#[test]
fn status_and_timeout_errors_are_typed_and_redacted() {
    for (status, expected_kind) in [
        ("401 Unauthorized", io::ErrorKind::PermissionDenied),
        ("404 Not Found", io::ErrorKind::Other),
        ("500 Internal Server Error", io::ErrorKind::Other),
    ] {
        let server = spawn_server(1, move |_index, _request, stream| {
            respond(stream, status, &[], b"BODY_SECRET")
        });
        let base = format!("{}/repo", server.base_url);
        let mut http = git_http();
        let mut response = http
            .get(
                &format!("{base}/info/refs?service=git-upload-pack&token=QUERY_SECRET"),
                &base,
                std::iter::empty::<&str>(),
            )
            .expect("prepare GET");
        let error = response.headers.fill_buf().expect_err("status must fail");
        assert_eq!(error.kind(), expected_kind);
        assert!(!error.to_string().contains("QUERY_SECRET"));
        assert!(!error.to_string().contains("BODY_SECRET"));
        server.finish();
    }

    let server = spawn_server(1, |_index, _request, stream| {
        thread::sleep(Duration::from_millis(200));
        let _ = ok(stream, b"late");
        Ok(())
    });
    let base = format!("{}/repo", server.base_url);
    let mut http = GitHttp::new(
        HttpClientPool::new(TEST_USER_AGENT)
            .git_blocking_client_builder()
            .timeout(Duration::from_millis(50)),
    )
    .expect("build short-timeout bridge");
    let mut response = http
        .get(
            &format!("{base}/info/refs?service=git-upload-pack&token=TIMEOUT_SECRET"),
            &base,
            std::iter::empty::<&str>(),
        )
        .expect("prepare timeout GET");
    let error = response
        .headers
        .fill_buf()
        .expect_err("request must time out");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "Git HTTP request timed out");
    server.finish();
}

#[test]
fn response_body_is_streamed_before_server_finishes() {
    let (first_chunk_tx, first_chunk_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = spawn_server(1, move |_index, _request, stream| {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n",
        )?;
        stream.flush()?;
        first_chunk_tx
            .send(())
            .map_err(|_| io::Error::other("first chunk receiver dropped"))?;
        release_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| io::Error::other("streaming test was not released"))?;
        stream.write_all(b"5\r\nworld\r\n0\r\n\r\n")?;
        stream.flush()
    });
    let base = format!("{}/repo", server.base_url);
    let mut http = git_http();
    let mut response = http
        .get(
            &format!("{base}/info/refs?service=git-upload-pack"),
            &base,
            std::iter::empty::<&str>(),
        )
        .expect("prepare streaming GET");
    response
        .headers
        .read_to_end(&mut Vec::new())
        .expect("read response headers");
    first_chunk_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first response chunk");

    let mut first = [0; 5];
    response
        .body
        .read_exact(&mut first)
        .expect("read first chunk");
    assert_eq!(&first, b"hello");
    release_tx.send(()).expect("release server");

    let mut rest = String::new();
    response
        .body
        .read_to_string(&mut rest)
        .expect("read remaining response");
    assert_eq!(rest, "world");
    server.finish();
}

#[test]
fn truncated_response_body_returns_a_safe_read_error() {
    let server = spawn_server(1, |_index, _request, stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabc")?;
        stream.flush()
    });
    let base = format!("{}/repo", server.base_url);
    let mut http = git_http();
    let mut response = http
        .get(
            &format!("{base}/info/refs?service=git-upload-pack"),
            &base,
            std::iter::empty::<&str>(),
        )
        .expect("prepare truncated GET");
    response
        .headers
        .read_to_end(&mut Vec::new())
        .expect("read response headers");
    let error = response
        .body
        .read_to_end(&mut Vec::new())
        .expect_err("truncated body must fail");
    assert_eq!(error.to_string(), "Git HTTP response body read failed");
    server.finish();
}

#[test]
fn pinned_gix_high_level_fetch_reaches_bounded_fetch_post() {
    let oid = "1111111111111111111111111111111111111111";
    let advertisement = packet_lines(&[b"version 2\n", b"ls-refs\n", b"fetch=shallow\n"]);
    let refs = packet_lines(&[format!("{oid} refs/heads/main\n").as_bytes()]);
    let server = spawn_server(3, move |index, _request, stream| match index {
        0 => respond(
            stream,
            "200 OK",
            &[(
                "Content-Type",
                "application/x-git-upload-pack-advertisement",
            )],
            &advertisement,
        ),
        1 => respond(
            stream,
            "200 OK",
            &[("Content-Type", "application/x-git-upload-pack-result")],
            &refs,
        ),
        _ => respond(stream, "500 Expected Test Stop", &[], b""),
    });
    let remote = format!("{}/repo", server.base_url);
    let repo_path = std::env::temp_dir().join(format!("tt-gix-fetch-{}", Uuid::new_v4()));
    let mut repo = gix::ThreadSafeRepository::init_opts(
        &repo_path,
        gix::create::Kind::Bare,
        gix::create::Options::default(),
        gix::open::Options::isolated(),
    )
    .expect("init bare repository")
    .to_thread_local();

    let result = fetch_exact(
        &mut repo,
        git_http(),
        &remote,
        "refs/heads/main",
        "refs/remotes/origin/main",
    );
    assert!(result.is_err(), "sentinel HTTP 500 must stop the fetch");

    let requests = (0..3)
        .map(|_| {
            server
                .requests
                .recv_timeout(Duration::from_secs(1))
                .expect("high-level smart HTTP request")
        })
        .collect::<Vec<_>>();
    assert!(
        requests[1]
            .body
            .windows(b"command=ls-refs".len())
            .any(|window| window == b"command=ls-refs")
    );
    assert!(
        requests[2]
            .body
            .windows(b"command=fetch".len())
            .any(|window| window == b"command=fetch")
    );

    server.finish();
    std::fs::remove_dir_all(repo_path).expect("remove test repository");
}

fn packet_lines(lines: &[&[u8]]) -> Vec<u8> {
    let mut output = Vec::new();
    for line in lines {
        write!(&mut output, "{:04x}", line.len() + 4).expect("write packet length");
        output.extend_from_slice(line);
    }
    output.extend_from_slice(b"0000");
    output
}
