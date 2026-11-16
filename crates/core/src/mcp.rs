//! A small Model Context Protocol server, enough for an AI CLI to call PuriCat's tools.
//!
//! JSON-RPC 2.0, one JSON object per line, over a Unix socket that exists for one chat
//! turn. The CLI cannot open a socket itself: it starts PuriCat's own executable in
//! [`bridge`] mode as a stdio MCP server, and the bridge relays to the socket. No TCP port
//! is opened, and the database connection never leaves the app.
//!
//! Only what a tool-calling client needs is implemented: `initialize`, `ping`,
//! `tools/list` and `tools/call`.

use std::future::Future;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// The name the tools are registered under in the CLI (`mcp__puricat__…` in Claude Code).
pub const SERVER_NAME: &str = "puricat";
/// Protocol revisions this server answers to; the first is offered to unknown clients.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-11-25", "2025-03-26", "2024-11-05"];
/// A request line longer than this ends the connection.
const MAX_LINE_BYTES: usize = 1 << 20;

pub struct ToolSpec {
    pub name: &'static str,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub input_schema: Value,
    /// The tool changes nothing, anywhere.
    pub read_only: bool,
}

pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

/// What stands behind the protocol: the tools and what calling one does.
pub trait ToolHost: Send + Sync + 'static {
    fn tools(&self) -> Vec<ToolSpec>;
    fn call(&self, name: &str, arguments: Value) -> impl Future<Output = ToolOutput> + Send;
}

/// Answer requests from one client until it disconnects.
pub async fn serve<R, W, H>(reader: R, mut writer: W, host: &H) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    H: ToolHost,
{
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = (&mut reader)
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        if read == 0 || line.len() > MAX_LINE_BYTES {
            return Ok(());
        }
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue; // not JSON: nothing to answer
        };
        if let Some(response) = respond(host, &message).await {
            let mut out = serde_json::to_vec(&response).unwrap_or_default();
            out.push(b'\n');
            writer.write_all(&out).await?;
            writer.flush().await?;
        }
    }
}

async fn respond<H: ToolHost>(host: &H, message: &Value) -> Option<Value> {
    let method = message.get("method")?.as_str()?;
    // Without an id it is a notification: there is nothing to answer.
    let id = message.get("id").filter(|id| !id.is_null())?.clone();
    let params = message.get("params");
    let outcome: Result<Value, (i64, String)> = match method {
        "initialize" => {
            let asked = params
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str);
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| Some(**v) == asked)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": host.tools().into_iter().map(|tool| json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
                "annotations": { "readOnlyHint": tool.read_only },
            })).collect::<Vec<_>>(),
        })),
        "tools/call" => {
            let name = params.and_then(|p| p.get("name")).and_then(Value::as_str);
            match name.filter(|name| host.tools().iter().any(|tool| tool.name == *name)) {
                Some(name) => {
                    let arguments = params
                        .and_then(|p| p.get("arguments"))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let output = host.call(name, arguments).await;
                    Ok(json!({
                        "content": [{ "type": "text", "text": output.text }],
                        "isError": output.is_error,
                    }))
                }
                None => Err((-32602, "Unknown tool".into())),
            }
        }
        _ => Err((-32601, "Method not found".into())),
    };
    Some(match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

/// The socket one chat turn's CLI reaches the tools through. Dropping it closes the socket,
/// ends every connection and removes the directory.
#[cfg(unix)]
pub struct Endpoint {
    dir: std::path::PathBuf,
    socket: std::path::PathBuf,
    accept: tokio::task::JoinHandle<()>,
}

#[cfg(unix)]
impl Endpoint {
    pub fn open<H: ToolHost>(host: std::sync::Arc<H>) -> std::io::Result<Endpoint> {
        use std::os::unix::fs::DirBuilderExt;
        // A directory of our own that only this user can enter; creating it fails if the
        // name is taken, so nobody can have put something there first.
        let name = uuid::Uuid::new_v4().simple().to_string();
        let dir = std::env::temp_dir().join(format!("puricat-{}", &name[..16]));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let socket = dir.join("mcp.sock");
        let listener = match tokio::net::UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e);
            }
        };
        let accept = tokio::spawn(async move {
            // Aborting this task drops the set, which ends every connection with it.
            let mut connections = tokio::task::JoinSet::new();
            while let Ok((stream, _)) = listener.accept().await {
                let host = host.clone();
                connections.spawn(async move {
                    let (reader, writer) = stream.into_split();
                    let _ = serve(reader, writer, &*host).await;
                });
            }
        });
        Ok(Endpoint {
            dir,
            socket,
            accept,
        })
    }

    pub fn socket(&self) -> &std::path::Path {
        &self.socket
    }
}

#[cfg(unix)]
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.accept.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Relay this process's stdin and stdout to an [`Endpoint`]. This is all the bridge mode of
/// the executable does: it holds no credentials and knows nothing about databases.
#[cfg(unix)]
pub fn bridge(socket: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut from_app = std::os::unix::net::UnixStream::connect(socket)?;
    let mut to_app = from_app.try_clone()?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut to_app);
        // The CLI is done: let the app see the end of the conversation.
        let _ = to_app.shutdown(std::net::Shutdown::Write);
    });
    let mut stdout = std::io::stdout().lock();
    std::io::copy(&mut from_app, &mut stdout)?;
    stdout.flush()
}

#[cfg(not(unix))]
pub fn bridge(_socket: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the AI tools are not available on this platform yet",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl ToolHost for Echo {
        fn tools(&self) -> Vec<ToolSpec> {
            vec![ToolSpec {
                name: "echo",
                description: "Says it back".into(),
                input_schema: json!({ "type": "object" }),
                read_only: true,
            }]
        }

        async fn call(&self, _name: &str, arguments: Value) -> ToolOutput {
            let text = arguments["text"].as_str().unwrap_or("").to_string();
            ToolOutput {
                is_error: text.is_empty(),
                text,
            }
        }
    }

    async fn exchange(input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out, &Echo).await.unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn answers_the_handshake_and_tool_calls() {
        // The lines are the ones Claude Code 2.1.303 and codex-cli 0.133.0 sent to a probe
        // server (client details shortened).
        let replies = exchange(concat!(
            r#"{"jsonrpc":"2.0","id":"server-discover-probe-1","method":"server/discover","params":{}}"#, "\n",
            r#"{"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{"roots":{"listChanged":true}},"clientInfo":{"name":"claude-code","version":"2.1.303"}},"jsonrpc":"2.0","id":0}"#, "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\n",
            r#"{"method":"tools/list","jsonrpc":"2.0","id":1}"#, "\n",
            r#"{"method":"tools/call","params":{"name":"echo","arguments":{"text":"สวัสดี"},"_meta":{"progressToken":2}},"jsonrpc":"2.0","id":2}"#, "\n",
            r#"{"method":"tools/call","params":{"name":"echo","arguments":{}},"jsonrpc":"2.0","id":3}"#, "\n",
            r#"{"method":"tools/call","params":{"name":"drop_everything","arguments":{}},"jsonrpc":"2.0","id":4}"#, "\n",
            r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#, "\n",
        ))
        .await;
        assert_eq!(replies.len(), 7, "the notification gets no answer");
        assert_eq!(replies[0]["id"], "server-discover-probe-1");
        assert_eq!(replies[0]["error"]["code"], -32601);
        assert_eq!(replies[1]["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(replies[1]["result"]["serverInfo"]["name"], "puricat");
        assert_eq!(replies[2]["result"]["tools"][0]["name"], "echo");
        assert_eq!(
            replies[2]["result"]["tools"][0]["annotations"]["readOnlyHint"],
            true
        );
        assert_eq!(replies[3]["result"]["content"][0]["text"], "สวัสดี");
        assert_eq!(replies[3]["result"]["isError"], false);
        assert_eq!(replies[4]["result"]["isError"], true);
        assert_eq!(
            replies[5]["error"]["code"], -32602,
            "an unknown tool is never called"
        );
        assert_eq!(replies[6]["result"], json!({}));
    }

    #[tokio::test]
    async fn survives_what_is_not_a_request() {
        let replies = exchange(concat!(
            "not json\n",
            "\n",
            r#"{"jsonrpc":"2.0","id":7,"result":{}}"#, "\n",
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#, "\n",
            r#"{"jsonrpc":"2.0","id":8,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#, "\n",
        ))
        .await;
        assert_eq!(replies.len(), 1);
        assert_eq!(
            replies[0]["result"]["protocolVersion"],
            PROTOCOL_VERSIONS[0]
        );

        // A line beyond the limit ends the connection instead of being buffered.
        let huge = format!(
            "{}\n{}\n",
            "x".repeat(MAX_LINE_BYTES + 10),
            r#"{"id":1,"method":"ping"}"#
        );
        assert!(exchange(&huge).await.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_endpoint_is_private_and_goes_away() {
        use std::os::unix::fs::PermissionsExt;
        let endpoint = Endpoint::open(std::sync::Arc::new(Echo)).unwrap();
        let socket = endpoint.socket().to_path_buf();
        let dir = socket.parent().unwrap().to_path_buf();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
        stream
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n")
            .await
            .unwrap();
        let mut reply = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut reply)
            .await
            .unwrap();
        assert!(reply.contains("\"result\":{}"), "{reply}");

        drop(endpoint);
        assert!(!dir.exists());
        // The open connection is ended too.
        let mut rest = Vec::new();
        let ended = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_to_end(&mut rest),
        )
        .await;
        assert!(ended.is_ok(), "the connection outlived the endpoint");
    }
}
