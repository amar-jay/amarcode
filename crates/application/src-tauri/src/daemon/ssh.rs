//! Native SSH forwarding. Settings persistence and renderer commands are separate.
use amarcode_protocol::rpc::HealthResult;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    sync::{watch, Mutex as AsyncMutex},
    task::JoinHandle,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const STDERR_LIMIT: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnelConfig {
    /// Hostname, IP address, or trusted alias from the user's SSH config.
    pub host: String,
    pub user: Option<String>,
    /// None preserves the SSH config's port (or SSH's default).
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    /// The authenticated WebSocket daemon on the SSH host's IPv4 loopback.
    pub daemon_port: u16,
}

impl SshTunnelConfig {
    fn validate(&self) -> Result<(), String> {
        if self.host.is_empty()
            || self.host.starts_with('-')
            || !self
                .host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-:".contains(&b))
        {
            return Err("Enter an SSH hostname, IP address, or SSH config alias".into());
        }
        if let Some(user) = &self.user {
            if user.is_empty()
                || user.starts_with('-')
                || !user
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err("Invalid SSH username".into());
            }
        }
        if self.port == Some(0) || self.daemon_port == 0 {
            return Err("SSH and daemon ports must be between 1 and 65535".into());
        }
        if let Some(path) = &self.identity_file {
            if !path.is_absolute() || !path.is_file() {
                return Err("Choose an existing SSH identity file using an absolute path".into());
            }
        }
        Ok(())
    }

    fn arguments(&self, local_port: u16) -> Vec<std::ffi::OsString> {
        let mut args = vec!["-N".into(), "-T".into(), "-n".into()];
        // Command-line settings override config defaults. User-defined ProxyJump
        // and identity configuration still work, but cannot weaken host checking.
        for option in [
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "ExitOnForwardFailure=yes",
            "ConnectTimeout=10",
            "ConnectionAttempts=1",
            "ServerAliveInterval=15",
            "ServerAliveCountMax=3",
            "ForwardAgent=no",
            "ForwardX11=no",
            "PermitLocalCommand=no",
            "RemoteCommand=none",
            "ControlMaster=no",
            "ControlPath=none",
            "ForkAfterAuthentication=no",
        ] {
            args.extend(["-o".into(), option.into()]);
        }
        if let Some(port) = self.port {
            args.extend(["-p".into(), port.to_string().into()]);
        }
        if let Some(user) = &self.user {
            args.extend(["-l".into(), user.into()]);
        }
        if let Some(path) = &self.identity_file {
            args.extend(["-i".into(), path.as_os_str().to_owned()]);
        }
        args.extend([
            "-L".into(),
            format!("127.0.0.1:{local_port}:127.0.0.1:{}", self.daemon_port).into(),
            self.host.clone().into(),
        ]);
        args
    }
}

/// Native-only endpoint; never persist the temporary forwarding port as identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshTunnelEndpoint {
    pub url: String,
}

struct Tunnel {
    config: SshTunnelConfig,
    // Used only to prevent reusing a tunnel verified with a different credential.
    token: String,
    endpoint: SshTunnelEndpoint,
    child: Child,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl Tunnel {
    fn diagnostic(&self) -> String {
        let bytes: Vec<u8> = self
            .stderr
            .lock()
            .map(|tail| tail.iter().copied().collect())
            .unwrap_or_default();
        String::from_utf8_lossy(&bytes).trim().to_owned()
    }

    async fn stop(&mut self) {
        // kill() both terminates and reaps; Drop remains the cancellation fallback.
        let _ = self.child.kill().await;
        if let Some(reader) = self.stderr_reader.take() {
            reader.abort();
            let _ = reader.await;
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        if let Some(reader) = &self.stderr_reader {
            reader.abort();
        }
        // Child also has kill_on_drop enabled, including during cancelled startup.
    }
}

/// Serializes startup so independent RPCs can share one owned SSH process.
/// A candidate replaces the current tunnel only after authenticated health succeeds.
pub struct SshTunnelManager {
    current: AsyncMutex<Option<Tunnel>>,
    cancellation: watch::Sender<u64>,
    executable: PathBuf,
    startup_timeout: Duration,
}

impl Default for SshTunnelManager {
    fn default() -> Self {
        Self {
            current: AsyncMutex::new(None),
            cancellation: watch::channel(0).0,
            executable: "ssh".into(),
            startup_timeout: STARTUP_TIMEOUT,
        }
    }
}

impl SshTunnelManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reuse a live tunnel, or verify a replacement without disturbing the old one.
    /// This does not persist settings, install software, or start the remote daemon.
    pub async fn connect(
        &self,
        config: &SshTunnelConfig,
        token: &str,
    ) -> Result<SshTunnelEndpoint, String> {
        config.validate()?;
        super::remote::validate("ws://127.0.0.1/rpc", token)?;
        let mut cancellation = self.cancellation.subscribe();
        let mut current = tokio::select! {
            guard = self.current.lock() => guard,
            _ = cancellation.changed() => return Err("SSH connection cancelled".into()),
        };
        if cancellation.has_changed().unwrap_or(true) {
            return Err("SSH connection cancelled".into());
        }
        if let Some(tunnel) = current.as_mut() {
            if tunnel.config == *config
                && tunnel.token == token
                && tunnel
                    .child
                    .try_wait()
                    .map_err(|e| format!("Cannot check SSH process: {e}"))?
                    .is_none()
            {
                return Ok(tunnel.endpoint.clone());
            }
        }
        let candidate = self.start(config, token);
        let tunnel = tokio::select! {
            result = candidate => result?,
            _ = cancellation.changed() => return Err("SSH connection cancelled".into()),
        };
        let endpoint = tunnel.endpoint.clone();
        let previous = current.replace(tunnel);
        // Replacement is committed before the first await, so cancellation cannot
        // drop the working candidate and leave a terminated previous tunnel active.
        if let Some(mut previous) = previous {
            previous.stop().await;
        }
        Ok(endpoint)
    }

    /// Cancels startup and terminates/reaps the owned tunnel, if present.
    pub async fn disconnect(&self) {
        self.cancellation
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        let mut current = self.current.lock().await;
        if let Some(mut tunnel) = current.take() {
            tunnel.stop().await;
        }
    }

    async fn start(&self, config: &SshTunnelConfig, token: &str) -> Result<Tunnel, String> {
        // OpenSSH needs a concrete local port. A competing bind after reservation
        // fails closed via ExitOnForwardFailure and authenticated readiness.
        let reservation = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("Cannot reserve SSH forwarding port: {e}"))?;
        let local_port = reservation.local_addr().map_err(|e| e.to_string())?.port();
        drop(reservation);
        let mut command = Command::new(&self.executable);
        command
            .args(config.arguments(local_port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = command.spawn().map_err(|e| {
            format!("Cannot start OpenSSH; ensure the ssh client is installed: {e}")
        })?;
        let mut stderr = child.stderr.take().expect("piped SSH stderr");
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        let reader_tail = Arc::clone(&tail);
        let stderr_reader = tokio::spawn(async move {
            let mut buffer = [0; 1024];
            while let Ok(n) = stderr.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                if let Ok(mut tail) = reader_tail.lock() {
                    tail.extend(&buffer[..n]);
                    let remove = tail.len().saturating_sub(STDERR_LIMIT);
                    tail.drain(..remove);
                }
            }
        });
        let mut tunnel = Tunnel {
            config: config.clone(),
            token: token.to_owned(),
            endpoint: SshTunnelEndpoint {
                url: format!("ws://127.0.0.1:{local_port}/rpc"),
            },
            child,
            stderr: tail,
            stderr_reader: Some(stderr_reader),
        };
        let ready = async {
            let mut last_error = "Waiting for SSH forwarding".to_owned();
            let deadline = tokio::time::Instant::now() + self.startup_timeout;
            loop {
                if let Some(status) = tunnel.child.try_wait().map_err(|e| e.to_string())? {
                    // EOF follows process exit, but allow the bounded reader to
                    // finish so host-key/authentication diagnostics are retained.
                    if let Some(mut reader) = tunnel.stderr_reader.take() {
                        let _ = tokio::time::timeout(Duration::from_millis(100), &mut reader).await;
                        reader.abort();
                    }
                    return Err(format!("SSH exited ({status}). {}", tunnel.diagnostic()));
                }
                match tokio::time::timeout_at(deadline, verify_daemon(&tunnel.endpoint.url, token))
                    .await
                {
                    Ok(Ok(())) => {
                        if tunnel
                            .child
                            .try_wait()
                            .map_err(|e| e.to_string())?
                            .is_none()
                        {
                            return Ok(());
                        }
                    }
                    Ok(Err(ProbeError::Fatal(error))) => return Err(error),
                    Ok(Err(ProbeError::Retry(error))) => last_error = error,
                    Err(_) => return Err(format!("SSH daemon connection timed out: {last_error}")),
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(format!("SSH daemon connection timed out: {last_error}"));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        .await;
        if let Err(error) = ready {
            tunnel.stop().await;
            return Err(error);
        }
        Ok(tunnel)
    }
}

enum ProbeError {
    Retry(String),
    Fatal(String),
}

async fn verify_daemon(url: &str, token: &str) -> Result<(), ProbeError> {
    let mut connection = super::bridge::Connection::remote(url, token)
        .await
        .map_err(ProbeError::Retry)?;
    let health: HealthResult = connection
        .call("health", ())
        .await
        .map_err(ProbeError::Retry)?;
    if !health.remote_auth_required {
        return Err(ProbeError::Fatal(
            "SSH target does not enforce daemon authentication".into(),
        ));
    }
    if health.status != "ok" || health.protocol_version != amarcode_protocol::PROTOCOL_VERSION {
        return Err(ProbeError::Fatal(format!(
            "SSH daemon must be healthy and use protocol {} (received {})",
            amarcode_protocol::PROTOCOL_VERSION,
            health.protocol_version
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
