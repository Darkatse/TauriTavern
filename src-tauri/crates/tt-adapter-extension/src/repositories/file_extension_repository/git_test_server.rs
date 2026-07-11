use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use gix::refs::Target;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit};

pub(super) struct GitTestServer {
    origin: PathBuf,
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    last_commits: HashMap<String, gix::ObjectId>,
}

impl GitTestServer {
    pub(super) fn start(root: PathBuf) -> Self {
        let version = Command::new("git")
            .arg("--version")
            .output()
            .expect("system Git is required for smart-HTTP integration tests");
        assert!(version.status.success(), "system Git must be runnable");

        std::fs::create_dir_all(&root).expect("create Git test server root");
        let origin = root.join("repo.git");
        gix::ThreadSafeRepository::init_opts(
            &origin,
            gix::create::Kind::Bare,
            gix::create::Options {
                destination_must_be_empty: Some(true),
                ..Default::default()
            },
            gix::open::Options::isolated().strict_config(true),
        )
        .expect("initialize test origin");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind Git test server");
        let address = listener.local_addr().expect("Git test server address");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_root = root.clone();
        let thread = thread::spawn(move || {
            for connection in listener.incoming() {
                let Ok(mut stream) = connection else {
                    break;
                };
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(error) = serve_request(&mut stream, &thread_root, address) {
                    let body = format!("Git test server error: {error}");
                    let _ = write!(
                        stream,
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                }
            }
        });

        Self {
            origin,
            address,
            stop,
            thread: Some(thread),
            last_commits: HashMap::new(),
        }
    }

    pub(super) fn remote_url(&self) -> String {
        format!("http://{}/repo.git", self.address)
    }

    pub(super) fn write_main(&mut self, version: &str) -> gix::ObjectId {
        self.write_branch("main", version)
    }

    pub(super) fn write_branch(&mut self, name: &str, version: &str) -> gix::ObjectId {
        let repo = gix::open_opts(
            &self.origin,
            gix::open::Options::isolated().strict_config(true),
        )
        .expect("open test origin");
        let manifest = serde_json::json!({
            "display_name": "Smart HTTP Fixture",
            "version": version,
            "author": "TauriTavern",
        });
        let manifest = serde_json::to_vec_pretty(&manifest).unwrap();
        let manifest = repo
            .write_blob(manifest)
            .expect("write manifest blob")
            .detach();
        let payload = repo
            .write_blob(format!("payload-{version}"))
            .expect("write payload blob")
            .detach();
        let mut entries = vec![
            gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: "manifest.json".into(),
                oid: manifest,
            },
            gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: "payload.txt".into(),
                oid: payload,
            },
        ];
        entries.sort();
        let tree = repo
            .write_object(&gix::objs::Tree { entries })
            .expect("write fixture tree")
            .detach();
        let actor = signature();
        let commit = repo
            .write_object(&gix::objs::Commit {
                tree,
                parents: self.last_commits.get(name).copied().into_iter().collect(),
                author: actor.clone(),
                committer: actor,
                encoding: None,
                message: format!("fixture {version}").into(),
                extra_headers: Vec::new(),
            })
            .expect("write fixture commit")
            .detach();

        let branch: gix::refs::FullName = format!("refs/heads/{name}").try_into().unwrap();
        let mut edits = vec![RefEdit {
            name: branch.clone(),
            deref: false,
            change: Change::Update {
                log: LogChange::default(),
                expected: PreviousValue::Any,
                new: Target::Object(commit),
            },
        }];
        if name == "main" {
            edits.push(RefEdit {
                name: "HEAD".try_into().unwrap(),
                deref: false,
                change: Change::Update {
                    log: LogChange::default(),
                    expected: PreviousValue::Any,
                    new: Target::Symbolic(branch),
                },
            });
        }
        let actor = signature();
        let mut time = gix::date::parse::TimeBuf::default();
        repo.edit_references_as(edits, Some(actor.to_ref(&mut time)))
            .expect("update fixture refs");
        self.last_commits.insert(name.to_string(), commit);
        commit
    }

    pub(super) fn point_branch(&mut self, name: &str, commit: gix::ObjectId) {
        let repo = gix::open_opts(
            &self.origin,
            gix::open::Options::isolated().strict_config(true),
        )
        .expect("open test origin");
        let edit = RefEdit {
            name: format!("refs/heads/{name}").try_into().unwrap(),
            deref: false,
            change: Change::Update {
                log: LogChange::default(),
                expected: PreviousValue::Any,
                new: Target::Object(commit),
            },
        };
        let actor = signature();
        let mut time = gix::date::parse::TimeBuf::default();
        repo.edit_references_as([edit], Some(actor.to_ref(&mut time)))
            .expect("update fixture branch ref");
        self.last_commits.insert(name.to_string(), commit);
    }

    pub(super) fn write_annotated_tag(&self, name: &str) {
        let commit = self.last_commits["main"];
        let repo = gix::open_opts(
            &self.origin,
            gix::open::Options::isolated().strict_config(true),
        )
        .expect("open test origin");
        let tag = repo
            .write_object(&gix::objs::Tag {
                target: commit,
                target_kind: gix::objs::Kind::Commit,
                name: name.into(),
                tagger: Some(signature()),
                message: format!("fixture tag {name}").into(),
                pgp_signature: None,
            })
            .expect("write annotated fixture tag")
            .detach();
        let edit = RefEdit {
            name: format!("refs/tags/{name}").try_into().unwrap(),
            deref: false,
            change: Change::Update {
                log: LogChange::default(),
                expected: PreviousValue::Any,
                new: Target::Object(tag),
            },
        };
        let actor = signature();
        let mut time = gix::date::parse::TimeBuf::default();
        repo.edit_references_as([edit], Some(actor.to_ref(&mut time)))
            .expect("update fixture tag ref");
    }
}

impl Drop for GitTestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn signature() -> gix::actor::Signature {
    gix::actor::Signature {
        name: "TauriTavern Test".into(),
        email: "test@tauritavern.local".into(),
        time: gix::date::Time::now_utc(),
    }
}

fn serve_request(
    stream: &mut TcpStream,
    project_root: &Path,
    address: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 8192];
    let header_end = loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err("request ended before headers".into());
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(position) = find_bytes(&request, b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = std::str::from_utf8(&request[..header_end])?.to_string();
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().ok_or("missing request line")?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().ok_or("missing method")?.to_string();
    let target = request_parts.next().ok_or("missing target")?.to_string();
    let mut content_length = 0_usize;
    let mut content_type = String::new();
    let mut git_protocol = String::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => content_length = value.trim().parse()?,
            "content-type" => content_type = value.trim().to_string(),
            "git-protocol" => git_protocol = value.trim().to_string(),
            _ => {}
        }
    }
    while request.len() < header_end + content_length {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err("request ended before body".into());
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let body = &request[header_end..header_end + content_length];
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));

    let mut child = Command::new("git")
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REQUEST_METHOD", &method)
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("CONTENT_TYPE", content_type)
        .env("CONTENT_LENGTH", content_length.to_string())
        .env("HTTP_GIT_PROTOCOL", &git_protocol)
        .env("GIT_PROTOCOL", &git_protocol)
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env("SERVER_NAME", address.ip().to_string())
        .env("SERVER_PORT", address.port().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.as_mut().unwrap().write_all(body)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "git http-backend failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let response_header_end = find_bytes(&output.stdout, b"\r\n\r\n")
        .map(|position| (position, 4))
        .or_else(|| find_bytes(&output.stdout, b"\n\n").map(|position| (position, 2)))
        .ok_or("CGI response has no header terminator")?;
    let raw_headers = std::str::from_utf8(&output.stdout[..response_header_end.0])?;
    let response_body = &output.stdout[response_header_end.0 + response_header_end.1..];
    let mut status = "200 OK".to_string();
    let mut forwarded = String::new();
    for line in raw_headers.lines() {
        if let Some(value) = line.strip_prefix("Status:") {
            status = value.trim().to_string();
        } else if !line.is_empty() {
            forwarded.push_str(line);
            forwarded.push_str("\r\n");
        }
    }
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{forwarded}Content-Length: {}\r\nConnection: close\r\n\r\n",
        response_body.len()
    )?;
    stream.write_all(response_body)?;
    Ok(())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
