//! The manager frame protocol, client side (`docs/05-mcp-bridge.md`): one JSON object per line,
//! `{"result": …}` or `{"error": {…}}`, a tokenless handshake first. The dashboard is just
//! another manager client; it never gets a private RPC surface (03).
//!
//! The protocol version is pinned here as data, not imported: this module is a separate
//! artifact and must be version-matched to the core it ships with (D5), so a core protocol
//! bump is a deliberate change in both places.

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::store::Connection;

const PROTOCOL_VERSION: &str = "0.1.0";
const MIN_SUPPORTED: &str = "0.1.0";
const SCOPE: &str = "dashboard";

pub struct FrameClient {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    token: String,
    scope_token: String,
}

impl FrameClient {
    pub async fn connect(connection: &Connection) -> Result<Self, String> {
        let stream = tokio::net::TcpStream::connect(&connection.address)
            .await
            .map_err(|err| format!("connect {}: {err}", connection.address))?;
        let (reader, writer) = stream.into_split();
        let mut client = Self {
            reader: BufReader::new(reader),
            writer,
            token: connection.token.clone(),
            scope_token: connection.scope_token.clone(),
        };
        client.handshake().await?;
        Ok(client)
    }

    async fn handshake(&mut self) -> Result<(), String> {
        let response = self
            .request(
                "handshake",
                Some(json!({"protocol_version": PROTOCOL_VERSION, "min_supported": MIN_SUPPORTED})),
                false,
            )
            .await?;
        let their = response
            .get("protocol_version")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if their.is_empty() {
            return Err("manager handshake carried no protocol version".into());
        }
        Ok(())
    }

    async fn request(
        &mut self,
        method: &str,
        params: Option<Value>,
        gated: bool,
    ) -> Result<Value, String> {
        let mut frame = json!({"method": method, "params": params});
        if gated {
            frame["token"] = Value::String(self.token.clone());
            frame["scope"] = Value::String(SCOPE.to_string());
            frame["scope_token"] = Value::String(self.scope_token.clone());
        }
        let mut line = serde_json::to_string(&frame).map_err(|err| format!("encode: {err}"))?;
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|err| format!("write: {err}"))?;
        self.writer
            .flush()
            .await
            .map_err(|err| format!("flush: {err}"))?;

        let mut response = String::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.reader.read_line(&mut response),
        )
        .await
        .map_err(|_| "manager did not answer within 15 s".to_string())?
        .map_err(|err| format!("read: {err}"))?;
        if read == 0 {
            return Err("manager closed the connection".into());
        }
        let value: Value = serde_json::from_str(&response)
            .map_err(|err| format!("malformed response frame: {err}"))?;
        if let Some(error) = value.get("error") {
            let code = error.get("code").and_then(Value::as_str).unwrap_or("error");
            let message = error.get("message").and_then(Value::as_str).unwrap_or("");
            return Err(format!("{code}: {message}"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| "response frame carried neither result nor error".to_string())
    }

    pub async fn call(&mut self, method: &str, params: Option<Value>) -> Result<Value, String> {
        self.request(method, params, true).await
    }
}

/// One-shot call: connect, handshake, call, drop. The dashboard polls on an interval, so a
/// connection per request keeps the module free of session state and stale sockets.
pub async fn call(
    connection: &Connection,
    method: &str,
    params: Option<Value>,
) -> Result<Value, String> {
    let mut client = FrameClient::connect(connection).await?;
    client.call(method, params).await
}

#[allow(dead_code)]
fn _keep_stream_type(_: TcpStream) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_one_json_object_per_line() {
        let frame = json!({"method": "status", "params": null});
        let line = serde_json::to_string(&frame).unwrap();
        assert!(!line.contains('\n'));
    }
}
