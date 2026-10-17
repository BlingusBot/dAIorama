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
    let (host, port) = match raw.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port = port.parse::<u16>().map_err(|_| invalid_bind())?;
            (host.to_string(), port)
        }
        None => (raw.to_string(), 8760),
        _ => return Err(invalid_bind()),
    };
    let loopback = host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "[::1]";
    if !loopback {
        return Err("dashboard serve is loopback-only in v1; use SSH forwarding".to_string());
    }
    Ok((host.trim_matches('#').to_string(), port))
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
    eprintln!("dashboard on http://{address}");
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let _ = handle(stream, app).await;
        });
    }
}

async fn handle(mut stream: TcpStream, app: App) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let read = stream.read(&mut buffer).await?;
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
    let mut content_length = 0usize;
    for header in parts {
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
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
        let read = stream.read(&mut buffer).await?;
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
