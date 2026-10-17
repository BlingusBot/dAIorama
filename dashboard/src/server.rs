//! The dashboard's local HTTP server (D3/D4): serves the three static files and a read JSON
//! API, holds the connection tokens server-side, and refuses anything that is not loopback.
//! The browser never sees a manager token; the only credential it can receive is a view's own
//! bootstrap URL from `get_view` (Q1).

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::client;
use crate::store::Connections;

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 4 * 1024;
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
/// 07's network shape, applied to the loopback UI too: a bounded connection count and a
/// per-read progress deadline, so a local page cannot park sockets or dribble a head forever.
const MAX_CONNECTIONS: usize = 64;
const PROGRESS: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone)]
pub struct App {
    pub connections: Arc<Connections>,
    pub ui: Arc<Ui>,
}

#[derive(Clone)]
pub struct Ui {
    pub index: String,
    pub css: String,
    pub js: String,
}

impl Ui {
    pub fn load(dir: &std::path::Path) -> Result<Self, String> {
        let read = |name: &str| -> Result<String, String> {
            std::fs::read_to_string(dir.join(name))
                .map_err(|err| format!("read {}: {err}", dir.join(name).display()))
        };
        Ok(Self {
            index: read("index.html")?,
            css: read("app.css")?,
            js: read("app.js")?,
        })
    }
}

/// Bind only loopback: the server holds every manager token and the UI has no user auth (Q2).
pub fn parse_bind(raw: &str) -> Result<(String, u16), String> {
    let raw = raw.trim();
    let (host, port) = if let Some(rest) = raw.strip_prefix('[') {
        // Bracketed IPv6 authority: `[::1]` or `[::1]:port`.
        let Some((host, tail)) = rest.split_once(']') else {
            return Err(invalid_bind());
        };
        if host.is_empty() {
            return Err(invalid_bind());
        }
        let port = match tail {
            "" => 8760,
            _ => match tail.strip_prefix(':') {
                Some(port) => port.parse::<u16>().map_err(|_| invalid_bind())?,
                None => return Err(invalid_bind()),
            },
        };
        (host.to_string(), port)
    } else {
        match raw.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => {
                let port = port.parse::<u16>().map_err(|_| invalid_bind())?;
                (host.to_string(), port)
            }
            None => (raw.to_string(), 8760),
            // A colon without a numeric port is a typo, or a bare IPv6 literal; brackets are
            // the unambiguous form.
            _ => return Err(invalid_bind()),
        }
    };
    let loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
    if !loopback {
        return Err("dashboard serve is loopback-only in v1; use SSH forwarding".to_string());
    }
    Ok((host, port))
}

fn invalid_bind() -> String {
    "dashboard bind takes host:port (loopback only, default 127.0.0.1:8760)".to_string()
}

pub async fn serve(app: App, host: &str, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind((host, port))
        .await
        .map_err(|err| format!("bind {host}:{port}: {err}"))?;
    let address = listener
        .local_addr()
        .map_err(|err| format!("bound address: {err}"))?;
    let allowed = allowed_authorities(&address);
    eprintln!("dashboard on http://{address}");
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let app = app.clone();
        let allowed = allowed.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = handle(stream, app, allowed).await;
        });
    }
}

/// The `Host` / `Origin` authorities this listener answers for: the bound address and
/// `localhost`, on the bound port. Anything else is a rebinding attempt, not this UI.
pub fn allowed_authorities(address: &std::net::SocketAddr) -> Vec<String> {
    let port = address.port();
    let host = match address.ip() {
        std::net::IpAddr::V4(ip) => ip.to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
    };
    let mut out = vec![format!("{host}:{port}"), format!("localhost:{port}")];
    if address.ip().is_loopback() {
        out.push(format!("127.0.0.1:{port}"));
        out.push(format!("[::1]:{port}"));
    }
    out.sort();
    out.dedup();
    out
}

fn host_allowed(headers: &Headers, allowed: &[String]) -> bool {
    headers
        .host
        .as_deref()
        .map(|host| allowed.iter().any(|entry| entry.eq_ignore_ascii_case(host)))
        .unwrap_or(false)
}

fn origin_allowed(headers: &Headers, allowed: &[String]) -> bool {
    let Some(origin) = headers.origin.as_deref() else {
        return true; // curl / same-origin navigations carry no Origin
    };
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    allowed.iter().any(|entry| entry.eq_ignore_ascii_case(rest))
}

#[derive(Default)]
struct Headers {
    host: Option<String>,
    origin: Option<String>,
    content_length: usize,
}

async fn read_bounded(stream: &mut TcpStream, buffer: &mut [u8]) -> std::io::Result<usize> {
    match tokio::time::timeout(PROGRESS, stream.read(buffer)).await {
        Ok(read) => read,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "request made no progress within 10 s",
        )),
    }
}

async fn handle(mut stream: TcpStream, app: App, allowed: Vec<String>) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let read = read_bounded(&mut stream, &mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        head.extend_from_slice(&buffer[..read]);
        if head.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if head.len() > MAX_HEAD {
            return reply(
                &mut stream,
                400,
                "application/json",
                &json!({"error": "head too large"}).to_string(),
            )
            .await;
        }
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let mut parts = text.split("\r\n");
    let request_line = parts.next().unwrap_or_default();
    let mut request = request_line.split_whitespace();
    let method = request.next().unwrap_or_default().to_string();
    let target = request.next().unwrap_or_default().to_string();
    let version = request.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return reply(
            &mut stream,
            400,
            "application/json",
            &json!({"error": "bad request line"}).to_string(),
        )
        .await;
    }
    let mut headers = Headers::default();
    for header in parts {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("host") {
            headers.host = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("origin") {
            headers.origin = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("content-length") {
            headers.content_length = value.parse().unwrap_or(0);
        }
    }
    // DNS rebinding: the browser is the tunnel, so `Host` must name this listener, and an
    // `Origin` that is present must be this UI (07's shape, applied to the loopback front).
    if !host_allowed(&headers, &allowed) {
        return reply(
            &mut stream,
            403,
            "application/json",
            &json!({"error": "host not allowed"}).to_string(),
        )
        .await;
    }
    if !origin_allowed(&headers, &allowed) {
        return reply(
            &mut stream,
            403,
            "application/json",
            &json!({"error": "origin not allowed"}).to_string(),
        )
        .await;
    }
    let content_length = headers.content_length;
    if content_length > MAX_BODY {
        return reply(
            &mut stream,
            413,
            "application/json",
            &json!({"error": "body too large"}).to_string(),
        )
        .await;
    }
    // Everything after the head terminator that isn't already read is the body.
    let head_end = head
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(head.len());
    let mut body = head[head_end..].to_vec();
    while body.len() < content_length {
        let read = read_bounded(&mut stream, &mut buffer).await?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }

    let (path, query) = split_target(&target);
    let result = route(&method, path, query, &body, &app).await;
    match result {
        Ok((status, content_type, payload)) => {
            reply(&mut stream, status, content_type, &payload).await
        }
        Err((status, message)) => {
            reply(
                &mut stream,
                status,
                "application/json",
                &json!({"error": message}).to_string(),
            )
            .await
        }
    }
}

fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

async fn route(
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    app: &App,
) -> Result<(u16, &'static str, String), (u16, String)> {
    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            Ok((200, "text/html; charset=utf-8", app.ui.index.clone()))
        }
        ("GET", "/app.css") => Ok((200, "text/css; charset=utf-8", app.ui.css.clone())),
        ("GET", "/app.js") => Ok((200, "text/javascript; charset=utf-8", app.ui.js.clone())),
        ("GET", "/api/managers") => Ok((200, "application/json", managers(app).await)),
        ("GET", "/api/sessions") => {
            let manager =
                param(query, "manager").ok_or((400, "manager is required".to_string()))?;
            let connection = app
                .connections
                .find(&manager)
                .ok_or((404, format!("unknown connection {manager:?}")))?;
            match timed(client::call(connection, "list_sessions", None)).await {
                Ok(value) => Ok((200, "application/json", value.to_string())),
                Err((status, error)) => Err((status, error)),
            }
        }
        ("GET", "/api/history") => {
            let manager =
                param(query, "manager").ok_or((400, "manager is required".to_string()))?;
            let session =
                param(query, "session").ok_or((400, "session is required".to_string()))?;
            let connection = app
                .connections
                .find(&manager)
                .ok_or((404, format!("unknown connection {manager:?}")))?;
            let mut params = json!({"session_id": session});
            if let Some(cursor) = param(query, "cursor") {
                params["cursor"] = Value::String(cursor);
            }
            if let Some(limit) = param(query, "limit") {
                params["limit"] = Value::Number(limit.parse::<u64>().unwrap_or(100).into());
            }
            match timed(client::call(connection, "list_history", Some(params))).await {
                Ok(value) => Ok((200, "application/json", value.to_string())),
                Err((status, error)) => Err((status, error)),
            }
        }
        ("POST", "/api/view") => {
            let payload: Value =
                serde_json::from_slice(body).map_err(|_| (400, "body must be JSON".to_string()))?;
            let manager = payload
                .get("manager")
                .and_then(Value::as_str)
                .ok_or((400, "manager is required".to_string()))?;
            let session = payload
                .get("session")
                .and_then(Value::as_str)
                .ok_or((400, "session is required".to_string()))?;
            let connection = app
                .connections
                .find(manager)
                .ok_or((404, format!("unknown connection {manager:?}")))?;
            let params = json!({"session_id": session});
            match timed(client::call(connection, "get_view", Some(params))).await {
                Ok(value) => Ok((200, "application/json", value.to_string())),
                Err((status, error)) => Err((status, error)),
            }
        }
        ("GET", _) => Err((404, "not found".to_string())),
        _ => Err((405, "method not allowed".to_string())),
    }
}

async fn managers(app: &App) -> String {
    let mut out = Vec::new();
    for connection in &app.connections.connections {
        match timed(client::call(connection, "readiness", None)).await {
            Ok(value) => out.push(json!({
                "name": connection.name,
                "address": connection.address,
                "ok": true,
                "readiness": value,
            })),
            Err((_, error)) => out.push(json!({
                "name": connection.name,
                "address": connection.address,
                "ok": false,
                "error": error,
            })),
        }
    }
    json!({"managers": out}).to_string()
}

async fn timed(
    future: impl std::future::Future<Output = Result<Value, String>>,
) -> Result<Value, (u16, String)> {
    match tokio::time::timeout(CALL_TIMEOUT, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err((502, error)),
        Err(_) => Err((502, "manager call timed out".to_string())),
    }
}

fn param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == name {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&raw[index + 1..index + 3], 16)
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

async fn reply(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        502 => "Bad Gateway",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_is_loopback_only() {
        assert_eq!(
            parse_bind("127.0.0.1:8760").unwrap(),
            ("127.0.0.1".to_string(), 8760)
        );
        assert_eq!(
            parse_bind("127.0.0.1").unwrap(),
            ("127.0.0.1".to_string(), 8760)
        );
        assert!(parse_bind("0.0.0.0:8760").is_err());
        assert!(parse_bind("10.0.0.5:8760").is_err());
        assert!(parse_bind("localhost:9000").is_ok());
        assert_eq!(
            parse_bind("[::1]:9000").unwrap(),
            ("::1".to_string(), 9000),
            "brackets are the authority form; the bind call gets the bare literal"
        );
        assert_eq!(parse_bind("[::1]").unwrap(), ("::1".to_string(), 8760));
    }

    #[test]
    fn only_loopback_authorities_are_served() {
        let allowed = vec!["127.0.0.1:8760".to_string(), "localhost:8760".to_string()];
        let mut headers = Headers {
            host: Some("127.0.0.1:8760".into()),
            ..Default::default()
        };
        assert!(host_allowed(&headers, &allowed));
        headers.host = Some("evil.example:8760".into());
        assert!(!host_allowed(&headers, &allowed));
        headers.host = None;
        assert!(!host_allowed(&headers, &allowed));

        assert!(
            origin_allowed(&Headers::default(), &allowed),
            "curl has no Origin"
        );
        let mut headers = Headers {
            origin: Some("http://127.0.0.1:8760".into()),
            ..Default::default()
        };
        assert!(origin_allowed(&headers, &allowed));
        headers.origin = Some("http://evil.example:8760".into());
        assert!(!origin_allowed(&headers, &allowed));
        headers.origin = Some("null".into());
        assert!(!origin_allowed(&headers, &allowed));
    }

    #[test]
    fn query_params_are_percent_decoded() {
        assert_eq!(
            param("manager=a&session=s%2D1", "session").as_deref(),
            Some("s-1")
        );
        assert_eq!(param("manager=a", "missing"), None);
    }
}
