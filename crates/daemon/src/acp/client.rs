//! High-level ACP client API over an agent process.
//!
//! Basic synchronous adapter: spawn an agent, exchange newline-delimited
//! JSON-RPC on stdio, correlate responses, and surface notifications on a
//! channel for `service::session` to drain.
//!
//! Owns process lifecycle + framing + id correlation. Does not touch SQLite
//! or TCP — `service::session` persists (`store`) and fans out after each unit.

use std::{
    collections::HashMap,
    fmt,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use serde_json::{json, Value};

use crate::protocol::{AgentEventMethod, AgentRpcMethod, RpcDirection, RpcEnvelope};

pub type AcpResult<T> = Result<T, AcpError>;

#[derive(Debug)]
pub enum AcpError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Timeout {
        id: u64,
    },
    ConnectionClosed,
    Protocol(String),
    Remote {
        code: Option<i64>,
        message: String,
        data: Option<Value>,
    },
}

impl fmt::Display for AcpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "ACP I/O error: {error}"),
            Self::Json(error) => write!(formatter, "ACP JSON error: {error}"),
            Self::Timeout { id } => write!(formatter, "ACP request {id} timed out"),
            Self::ConnectionClosed => write!(formatter, "ACP connection closed"),
            Self::Protocol(message) => write!(formatter, "ACP protocol error: {message}"),
            Self::Remote { code, message, .. } => {
                write!(formatter, "ACP remote error {code:?}: {message}")
            }
        }
    }
}

impl std::error::Error for AcpError {}

impl From<std::io::Error> for AcpError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for AcpError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// An incoming agent notification or an agent-initiated request that needs a
/// JSON-RPC response from the client.
#[derive(Debug, Clone)]
pub enum AcpInbound {
    Notification {
        event: AgentEventMethod,
        envelope: RpcEnvelope,
    },
    Request {
        id: u64,
        method: String,
        params: Value,
    },
    InvalidMessage {
        error: String,
        raw: String,
    },
    Disconnected,
    /// Internal queue barrier used to wait until all preceding inbound traffic
    /// has been handled by the session worker.
    Barrier(Sender<()>),
}

type PendingResponse = Sender<AcpResult<Value>>;

pub struct AcpClient {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
    inbound_sender: Sender<AcpInbound>,
}

impl AcpClient {
    /// Starts an ACP adapter that exchanges one JSON-RPC message per stdout line.
    ///
    /// The returned receiver must be drained by the daemon's event loop.
    /// `cwd` is the workspace directory when known.
    pub fn spawn(
        command: &str,
        arguments: &[String],
        environment: &[(String, String)],
        cwd: Option<&Path>,
    ) -> AcpResult<(Self, Receiver<AcpInbound>)> {
        let mut process = Command::new(command);
        process
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (key, value) in environment {
            process.env(key, value);
        }
        if let Some(dir) = cwd {
            process.current_dir(dir);
        }

        let mut child = process.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AcpError::Protocol("agent stdin was not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AcpError::Protocol("agent stdout was not piped".into()))?;
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (inbound_sender, inbound_receiver) = mpsc::channel();

        spawn_reader(stdout, Arc::clone(&pending), inbound_sender.clone());

        Ok((
            Self {
                stdin: Mutex::new(stdin),
                child: Mutex::new(child),
                next_id: AtomicU64::new(1),
                pending,
                inbound_sender,
            },
            inbound_receiver,
        ))
    }

    /// Sends a JSON-RPC request and waits for its result. Incoming notifications
    /// continue arriving through the receiver returned by [`Self::spawn`].
    pub fn request(
        &self,
        method: AgentRpcMethod,
        params: Value,
        timeout: Duration,
    ) -> AcpResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.pending.lock().map_err(lock_error)?.insert(id, sender);

        if let Err(error) = self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method.as_str(),
            "params": params,
        })) {
            self.pending.lock().map_err(lock_error)?.remove(&id);
            return Err(error);
        }

        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending.lock().map_err(lock_error)?.remove(&id);
                Err(AcpError::Timeout { id })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AcpError::ConnectionClosed),
        }
    }

    /// Sends a JSON-RPC notification, which has no response ID.
    pub fn notify(&self, method: AgentRpcMethod, params: Value) -> AcpResult<()> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": method.as_str(),
            "params": params,
        }))
    }

    /// Answers an agent-initiated JSON-RPC request delivered as `AcpInbound::Request`.
    pub fn respond(&self, id: u64, result: Value) -> AcpResult<()> {
        self.write_message(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    pub fn respond_error(
        &self,
        id: u64,
        code: i64,
        message: &str,
        data: Option<Value>,
    ) -> AcpResult<()> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message, "data": data },
        }))
    }

    /// Stops the child process. Prefer `agent.shutdown` first when the adapter
    /// supports a graceful shutdown request.
    pub fn kill(&self) -> AcpResult<()> {
        self.child.lock().map_err(lock_error)?.kill()?;
        Ok(())
    }

    pub fn try_wait(&self) -> AcpResult<Option<std::process::ExitStatus>> {
        Ok(self.child.lock().map_err(lock_error)?.try_wait()?)
    }

    /// Wait until the session worker has processed inbound events that arrived
    /// before this call.
    pub fn sync_inbound(&self, timeout: Duration) -> AcpResult<()> {
        let (sender, receiver) = mpsc::channel();
        self.inbound_sender
            .send(AcpInbound::Barrier(sender))
            .map_err(|_| AcpError::ConnectionClosed)?;
        receiver.recv_timeout(timeout).map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => AcpError::Protocol("inbound sync timed out".into()),
            mpsc::RecvTimeoutError::Disconnected => AcpError::ConnectionClosed,
        })
    }

    fn write_message(&self, message: &Value) -> AcpResult<()> {
        let mut stdin = self.stdin.lock().map_err(lock_error)?;
        serde_json::to_writer(&mut *stdin, message)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_reader(
    stdout: impl std::io::Read + Send + 'static,
    pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
    inbound_sender: Sender<AcpInbound>,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => route_incoming(&line, &pending, &inbound_sender),
                Err(error) => {
                    let _ = inbound_sender.send(AcpInbound::InvalidMessage {
                        error: error.to_string(),
                        raw: String::new(),
                    });
                    break;
                }
            }
        }

        if let Ok(mut waiting) = pending.lock() {
            for (_, sender) in waiting.drain() {
                let _ = sender.send(Err(AcpError::ConnectionClosed));
            }
        }
        let _ = inbound_sender.send(AcpInbound::Disconnected);
    });
}

fn route_incoming(
    raw: &str,
    pending: &Arc<Mutex<HashMap<u64, PendingResponse>>>,
    inbound_sender: &Sender<AcpInbound>,
) {
    let message: Value = match serde_json::from_str(raw) {
        Ok(message) => message,
        Err(error) => {
            let _ = inbound_sender.send(AcpInbound::InvalidMessage {
                error: error.to_string(),
                raw: raw.into(),
            });
            return;
        }
    };

    if let Some(method) = message.get("method").and_then(Value::as_str) {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            let _ = inbound_sender.send(AcpInbound::Request {
                id,
                method: method.into(),
                params,
            });
        } else {
            let _ = inbound_sender.send(AcpInbound::Notification {
                event: AgentEventMethod::from(method),
                envelope: RpcEnvelope {
                    direction: RpcDirection::Received,
                    method: method.into(),
                    payload: params,
                },
            });
        }
        return;
    }

    let Some(id) = message.get("id").and_then(Value::as_u64) else {
        let _ = inbound_sender.send(AcpInbound::InvalidMessage {
            error: "message has neither method nor numeric id".into(),
            raw: raw.into(),
        });
        return;
    };

    let result = if let Some(error) = message.get("error") {
        Err(AcpError::Remote {
            code: error.get("code").and_then(Value::as_i64),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown remote error")
                .into(),
            data: error.get("data").cloned(),
        })
    } else if let Some(result) = message.get("result") {
        Ok(result.clone())
    } else {
        Err(AcpError::Protocol(
            "response has neither result nor error".into(),
        ))
    };

    match pending.lock() {
        Ok(mut waiting) => {
            if let Some(sender) = waiting.remove(&id) {
                let _ = sender.send(result);
            }
        }
        Err(_) => {
            let _ = inbound_sender.send(AcpInbound::InvalidMessage {
                error: "pending response lock poisoned".into(),
                raw: raw.into(),
            });
        }
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> AcpError {
    AcpError::Protocol("ACP internal lock poisoned".into())
}
