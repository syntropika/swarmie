//! Bounded static preview files with a direct loopback server and scoped daemon relay.
use crate::{WorkspaceExtension, assets, relative, required};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use swarmie::{Error, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task::{JoinHandle, JoinSet},
};
struct Preview {
    url: String,
    root: PathBuf,
    task: JoinHandle<()>,
}
#[derive(Default)]
pub struct Previews(Mutex<BTreeMap<String, Preview>>, AtomicBool);
impl Drop for Previews {
    fn drop(&mut self) {
        for preview in self.0.get_mut().values() {
            preview.task.abort();
        }
    }
}
impl Previews {
    pub async fn shutdown(&self) {
        self.1.store(true, Ordering::SeqCst);
        for (_, preview) in std::mem::take(&mut *self.0.lock().await) {
            preview.task.abort();
        }
    }
    pub async fn invoke(
        &self,
        extension: &WorkspaceExtension,
        command: &str,
        p: &Value,
    ) -> Result<Value> {
        if self.1.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        if command == "preview.start" {
            let project = extension.project(p).await?;
            let name = p["path"].as_str().unwrap_or("");
            if name
                .split('/')
                .any(|part| part.starts_with('.') && part != ".")
            {
                return Err(Error::Invalid(
                    "preview root must not be a private directory".into(),
                ));
            }
            let root = extension.path(&project, name).await?;
            if private_path(&project, &root) {
                return Err(Error::Invalid(
                    "preview root must not be a private directory".into(),
                ));
            }
            if !root.is_dir() {
                return Err(Error::Invalid("preview root must be a folder".into()));
            }
            let mut previews = self.0.lock().await;
            if self.1.load(Ordering::SeqCst) {
                return Err(Error::Closed);
            }
            if previews.len() >= 8 {
                return Err(Error::Invalid(
                    "close an existing preview before starting another".into(),
                ));
            }
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let url = format!("http://{address}");
            let host = address.to_string();
            let directory = root.clone();
            let task = tokio::spawn(async move {
                let mut clients = JoinSet::new();
                loop {
                    tokio::select! {
                        connection = listener.accept() => match connection {
                            Ok((socket, _)) if clients.len() < 32 => {
                                let root = directory.clone(); let host = host.clone();
                                clients.spawn(async move { let _ = tokio::time::timeout(Duration::from_secs(3), serve(socket, root, &host)).await; });
                            }
                            Ok(_) => (), Err(_) => break,
                        },
                        _ = clients.join_next(), if !clients.is_empty() => (),
                    }
                }
            });
            let id = uuid::Uuid::new_v4().to_string();
            previews.insert(
                id.clone(),
                Preview {
                    url: url.clone(),
                    root: root.clone(),
                    task,
                },
            );
            return Ok(
                json!({"previewId":id,"url":url,"relayPath":format!("/preview/{id}/"),"path":root}),
            );
        }
        let id = required(p, "previewId")?;
        let mut previews = self.0.lock().await;
        if command == "preview.stop" {
            let preview = previews
                .remove(id)
                .ok_or_else(|| Error::Invalid("unknown preview".into()))?;
            preview.task.abort();
            return Ok(json!({"stopped":true}));
        }
        let preview = previews
            .get(id)
            .ok_or_else(|| Error::Invalid("unknown preview".into()))?;
        if command == "preview.read" {
            if preview.task.is_finished() {
                return Err(Error::Closed);
            }
            let root = preview.root.clone();
            drop(previews);
            let prefix = format!("/preview/{id}/");
            let response =
                read_file(&root, p["path"].as_str().unwrap_or(""), Some(&prefix)).await?;
            return Ok(
                json!({"status":response.status,"mime":response.mime,"data":STANDARD.encode(response.body),"location":response.location}),
            );
        }
        if command != "preview.status" {
            return Err(Error::Unsupported(command.into()));
        }
        Ok(
            json!({"previewId":id,"url":preview.url,"relayPath":format!("/preview/{id}/"),"path":preview.root,"running":!preview.task.is_finished()}),
        )
    }
}
fn private_path(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative
            .components()
            .any(|part| part.as_os_str().as_encoded_bytes().starts_with(b"."))
    })
}
struct StaticResponse {
    status: u16,
    mime: &'static str,
    body: Vec<u8>,
    location: Option<String>,
}
impl StaticResponse {
    fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            mime: "text/plain",
            body: message.as_bytes().to_vec(),
            location: None,
        }
    }
}
async fn read_file(root: &Path, target: &str, prefix: Option<&str>) -> Result<StaticResponse> {
    if target.len() > 4096 {
        return Ok(StaticResponse::error(414, "Preview path too long"));
    }
    let target = target
        .split('?')
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    let decoded = match percent_encoding::percent_decode_str(target).decode_utf8() {
        Ok(path) => path,
        Err(_) => return Ok(StaticResponse::error(400, "Invalid preview path")),
    };
    if relative(&decoded).is_err()
        || decoded.contains(['\\', '\0'])
        || decoded.split('/').any(|part| part.starts_with('.'))
    {
        return Ok(StaticResponse::error(403, "Private path"));
    }
    let requested = root.join(decoded.as_ref());
    let mut path = match tokio::fs::canonicalize(requested).await {
        Ok(path) if path.starts_with(root) && private_path(root, &path) => {
            return Ok(StaticResponse::error(403, "Private path"));
        }
        Ok(path) if path.starts_with(root) => path,
        _ => return Ok(StaticResponse::error(404, "Not found")),
    };
    if path.is_dir() {
        if !decoded.is_empty() && !target.ends_with('/') {
            return Ok(StaticResponse {
                status: 308,
                mime: "text/plain",
                body: vec![],
                location: Some(format!("{}{target}/", prefix.unwrap_or("/"))),
            });
        }
        path = match tokio::fs::canonicalize(path.join("index.html")).await {
            Ok(path) if path.starts_with(root) && private_path(root, &path) => {
                return Ok(StaticResponse::error(403, "Private path"));
            }
            Ok(path) if path.starts_with(root) => path,
            _ => return Ok(StaticResponse::error(404, "Not found")),
        };
    }
    let meta = tokio::fs::metadata(&path).await?;
    if !meta.is_file() {
        return Ok(StaticResponse::error(
            403,
            "Preview requires a regular file",
        ));
    }
    if meta.len() > assets::MAX_ASSET {
        return Ok(StaticResponse::error(413, "Preview file exceeds 8 MiB"));
    }
    let mut bytes = vec![];
    tokio::fs::File::open(&path)
        .await?
        .take(assets::MAX_ASSET + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > assets::MAX_ASSET {
        return Ok(StaticResponse::error(413, "Preview file exceeds 8 MiB"));
    }
    let mime = assets::mime(&path, &bytes);
    if let Some(prefix) = prefix
        && matches!(mime, "text/html" | "text/css" | "text/javascript")
    {
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => return Ok(StaticResponse::error(415, "Preview source must be UTF-8")),
        };
        bytes = match scoped_paths(text, prefix, mime) {
            Some(text) => text.into_bytes(),
            None => {
                return Ok(StaticResponse::error(
                    413,
                    "Rewritten preview exceeds 8 MiB",
                ));
            }
        };
    }
    Ok(StaticResponse {
        status: 200,
        mime,
        body: bytes,
        location: None,
    })
}
// Root-relative URL literals in HTML attributes, CSS url()/imports and bundled JavaScript
// share one small lexical rule. Relative paths and protocol-relative/external URLs stay intact.
fn js_roots(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut roots = vec![];
    let mut index = 0;
    let mut regex = true;
    let mut control = false;
    let mut parentheses = vec![];
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
                continue;
            }
            quote @ (b'\'' | b'"' | b'`') => {
                index += 1;
                if bytes.get(index) == Some(&b'/') && bytes.get(index + 1) != Some(&b'/') {
                    roots.push(index);
                }
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                        continue;
                    }
                    if bytes[index] == quote {
                        index += 1;
                        break;
                    }
                    index += 1;
                }
                regex = false;
                continue;
            }
            b'/' if regex => {
                index += 1;
                let mut class = false;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => {
                            index = (index + 2).min(bytes.len());
                            continue;
                        }
                        b'[' => class = true,
                        b']' => class = false,
                        b'/' if !class => {
                            index += 1;
                            break;
                        }
                        b'\n' | b'\r' => break,
                        _ => (),
                    }
                    index += 1;
                }
                regex = false;
                continue;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'$'))
                {
                    index += 1;
                }
                let word = &source[start..index];
                control = matches!(word, "if" | "while" | "for" | "with" | "switch" | "catch");
                regex = control
                    || matches!(
                        word,
                        "return"
                            | "throw"
                            | "case"
                            | "delete"
                            | "void"
                            | "typeof"
                            | "new"
                            | "yield"
                            | "await"
                            | "else"
                            | "do"
                            | "in"
                            | "instanceof"
                    );
                continue;
            }
            b'(' => {
                parentheses.push(control);
                control = false;
                regex = true;
            }
            b')' => regex = parentheses.pop().unwrap_or(false),
            b']' => regex = false,
            byte if byte.is_ascii_whitespace() => (),
            byte if byte.is_ascii_digit() || byte == b'.' => regex = false,
            _ => regex = true,
        }
        index += 1;
    }
    roots
}
fn scoped_paths(source: &str, prefix: &str, mime: &str) -> Option<String> {
    let bytes = source.as_bytes();
    let mut indices = vec![];
    let mut script_ranges = vec![];
    if mime == "text/html" {
        let lower = source.to_ascii_lowercase();
        let mut offset = 0;
        while let Some(start) = lower[offset..].find("<script") {
            let start = offset + start;
            let Some(content) = lower[start..].find('>') else {
                break;
            };
            let content = start + content + 1;
            let Some(end) = lower[content..].find("</script") else {
                break;
            };
            let end = content + end;
            indices.extend(
                js_roots(&source[content..end])
                    .into_iter()
                    .map(|index| content + index),
            );
            script_ranges.push(content..end);
            offset = end + 8;
        }
    }
    if mime == "text/javascript" {
        indices = js_roots(source);
    } else {
        let mut inside_tag = false;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'<' {
                inside_tag = true;
            }
            if *byte == b'>' {
                inside_tag = false;
            }
            if *byte != b'/'
                || bytes.get(index + 1) == Some(&b'/')
                || script_ranges.iter().any(|range| range.contains(&index))
            {
                continue;
            }
            let previous = bytes[..index]
                .iter()
                .rposition(|byte| !byte.is_ascii_whitespace());
            let preceding = previous.map(|position| bytes[position]);
            let quoted = matches!(preceding, Some(b'\'' | b'"' | b'`'));
            let attribute = mime == "text/html" && inside_tag && preceding == Some(b'=');
            let css_url = mime == "text/css"
                && preceding == Some(b'(')
                && previous.is_some_and(|position| {
                    bytes
                        .get(position.saturating_sub(3)..position)
                        .is_some_and(|name| name.eq_ignore_ascii_case(b"url"))
                });
            if quoted || attribute || css_url {
                indices.push(index);
            }
        }
    }
    indices.sort_unstable();
    indices.dedup();
    let prefix = prefix.trim_end_matches('/');
    let size = source
        .len()
        .checked_add(indices.len().checked_mul(prefix.len())?)?;
    if size > assets::MAX_ASSET as usize {
        return None;
    }
    let mut rewritten = String::with_capacity(size);
    let mut start = 0;
    for index in indices {
        rewritten.push_str(&source[start..index]);
        rewritten.push_str(prefix);
        start = index;
    }
    rewritten.push_str(&source[start..]);
    Some(rewritten)
}

async fn reply(socket: &mut TcpStream, response: StaticResponse) -> Result<()> {
    let reason = match response.status {
        200 => "OK",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        _ => "Request Header Fields Too Large",
    };
    let location = response
        .location
        .map(|location| format!("Location: {location}\r\n"))
        .unwrap_or_default();
    socket.write_all(format!("HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{location}Connection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n",response.status,response.mime,response.body.len()).as_bytes()).await?;
    socket.write_all(&response.body).await?;
    Ok(())
}
async fn serve(mut socket: TcpStream, root: PathBuf, host: &str) -> Result<()> {
    let mut request = vec![];
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|b| b == b"\r\n\r\n") {
        let n = socket.read(&mut buffer).await?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..n]);
        if request.len() > 16 * 1024 {
            return reply(&mut socket, StaticResponse::error(431, "Request too large")).await;
        }
    }
    let request = std::str::from_utf8(&request)
        .map_err(|_| Error::Invalid("invalid preview request".into()))?;
    let supplied_host = request.lines().find_map(|line| {
        line.split_once(':')
            .filter(|(key, _)| key.eq_ignore_ascii_case("host"))
            .map(|(_, v)| v.trim())
    });
    if supplied_host != Some(host) {
        return reply(&mut socket, StaticResponse::error(403, "Invalid host")).await;
    }
    let mut fields = request.lines().next().unwrap_or("").split_whitespace();
    if fields.next() != Some("GET") {
        return reply(&mut socket, StaticResponse::error(405, "Use GET")).await;
    }
    let target = fields.next().unwrap_or("/");
    reply(&mut socket, read_file(&root, target, None).await?).await
}
