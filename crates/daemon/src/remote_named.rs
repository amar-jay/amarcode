//! Persistent installation enrollment. Cloudflare account credentials never leave the Worker.
use crate::{Config, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{IsTerminal, Write},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

const ALLOCATOR: &str = "https://updates.amarcode.amarjay.com";
#[derive(Serialize, Deserialize)]
struct Identity {
    id: String,
    credential: String,
}
#[derive(Deserialize)]
struct Assignment {
    installation_id: String,
    hostname: String,
    url: String,
    tunnel_token: String,
    port: u16,
    #[serde(default)]
    dns_ready_after: Option<u64>,
}
fn root() -> Result<PathBuf> {
    std::env::var_os("AMARCODE_REMOTE_ROOT")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(crate::remote::instance_root)
}
fn identity_path() -> Result<PathBuf> {
    Ok(root()?.join("named-installation.json"))
}
fn private_read(path: &std::path::Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::msg(
            "Named installation identity must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(Error::msg(
                "Named installation identity must have mode 0600",
            ));
        }
    }
    Ok(fs::read(path)?)
}
fn identity(create: bool) -> Result<Identity> {
    identity_at(&identity_path()?, create)
}
fn identity_at(path: &std::path::Path, create: bool) -> Result<Identity> {
    if !path.try_exists()? {
        if !create {
            return Err(Error::msg("No named tunnel installation is enrolled"));
        }
        fs::create_dir_all(path.parent().unwrap())?;
        let value = Identity {
            id: uuid::Uuid::new_v4().to_string(),
            credential: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        };
        // NamedTempFile is private; no partially-written or world-readable credential.
        let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        temporary.write_all(&serde_json::to_vec(&value)?)?;
        temporary.as_file().sync_all()?;
        if let Err(error) = temporary.persist_noclobber(path) {
            if error.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error.error.into());
            }
        }
    }
    #[cfg(unix)]
    fs::File::open(path.parent().unwrap())?.sync_all()?;
    let identity: Identity = serde_json::from_slice(&private_read(path)?)?;
    if uuid::Uuid::parse_str(&identity.id).is_err()
        || identity.credential.len() != 64
        || !identity.credential.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(Error::msg("Invalid named installation identity"));
    }
    Ok(identity)
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| Error::msg("Cannot initialize tunnel HTTP client"))
}
async fn request(
    method: reqwest::Method,
    identity: &Identity,
    port: Option<u16>,
) -> Result<reqwest::Response> {
    let mut request = client()?
        .request(method, format!("{ALLOCATOR}/v1/tunnels/{}", identity.id))
        .bearer_auth(&identity.credential)
        .header("x-amarcode-tunnel-hostnames", "short-v2");
    if let Some(port) = port {
        request = request
            .body(serde_json::json!({"port": port}).to_string())
            .header("content-type", "application/json");
    }
    let response = request
        .send()
        .await
        .map_err(|_| Error::msg("Cannot reach the named tunnel allocator"))?;
    match response.status().as_u16() {
        200..=299 => Ok(response),
        401 => Err(Error::msg("Named installation credential rejected")),
        403 => Err(Error::msg(
            "Named tunnel allocation was denied by the server",
        )),
        409 => Err(Error::msg("Named tunnel installation capacity reached")),
        410 => Err(Error::msg(
            "Named installation was removed; run amarcoded tunnel --rm to reset local enrollment",
        )),
        503 => Err(Error::msg(
            "Named tunnel allocator is not configured on the server",
        )),
        _ => Err(Error::msg(
            "Named tunnel provisioning failed; retry with the same installation identity",
        )),
    }
}
async fn allocate(port: u16) -> Result<Assignment> {
    let identity = identity(true)?;
    let assignment: Assignment = serde_json::from_slice(
        &request(reqwest::Method::PUT, &identity, Some(port))
            .await?
            .bytes()
            .await
            .map_err(|_| Error::msg("Cannot read tunnel allocation response"))?,
    )?;
    validate_assignment(&assignment, &identity, port)?;
    Ok(assignment)
}
fn validate_assignment(value: &Assignment, identity: &Identity, port: u16) -> Result<()> {
    let url =
        reqwest::Url::parse(&value.url).map_err(|_| Error::msg("Invalid allocated tunnel URL"))?;
    let short_hostname = [".amarjay.com", ".amarcode.amarjay.com"]
        .iter()
        .any(|suffix| {
            value.hostname.strip_suffix(suffix).is_some_and(|label| {
                label.len() == 8
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        });
    let legacy_hostname = value.hostname.starts_with(&format!("m-{}.", identity.id));
    if value.installation_id != identity.id
        || value.port != port
        || url.scheme() != "wss"
        || url.host_str() != Some(value.hostname.as_str())
        || url.path() != "/rpc"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !(short_hostname || legacy_hostname)
        || value.tunnel_token.is_empty()
        || value.tunnel_token.len() > 8192
    {
        return Err(Error::msg("Invalid named tunnel assignment"));
    }
    Ok(())
}

pub async fn remove() -> Result<()> {
    let identity = identity(false)?;
    request(reqwest::Method::DELETE, &identity, None).await?;
    fs::remove_file(identity_path()?)?;
    println!("Named tunnel removed. Its hostname will never be reassigned.");
    Ok(())
}

struct TunnelProgress {
    message: Arc<Mutex<Option<String>>>,
    animation: Option<tokio::task::JoinHandle<()>>,
}

impl TunnelProgress {
    fn new() -> Self {
        let message = Arc::new(Mutex::new(None::<String>));
        let animation =
            if std::io::stderr().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb") {
                let state = Arc::clone(&message);
                Some(tokio::spawn(async move {
                    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                    let started = tokio::time::Instant::now();
                    let mut frame = 0;
                    let mut ticks = tokio::time::interval(Duration::from_millis(90));
                    loop {
                        ticks.tick().await;
                        // Rendering and clearing share this lock: no frame can be
                        // written after finish(), including on errors or Ctrl+C.
                        let current = state.lock().unwrap();
                        if let Some(text) = current.as_ref() {
                            let mut stderr = std::io::stderr().lock();
                            let _ = write!(
                                stderr,
                                "\r\x1b[2K{} {} ({:.0}s)",
                                frames[frame % frames.len()],
                                text,
                                started.elapsed().as_secs_f64()
                            );
                            let _ = stderr.flush();
                            frame += 1;
                        }
                    }
                }))
            } else {
                None
            };
        Self { message, animation }
    }

    fn set(&self, text: &str) {
        if self.animation.is_some() {
            *self.message.lock().unwrap() = Some(text.into());
        } else {
            eprintln!("{text}");
        }
    }

    fn finish(&self) {
        let mut current = self.message.lock().unwrap();
        *current = None;
        if let Some(animation) = &self.animation {
            animation.abort();
            let mut stderr = std::io::stderr().lock();
            let _ = write!(stderr, "\r\x1b[2K");
            let _ = stderr.flush();
        }
    }
}

impl Drop for TunnelProgress {
    fn drop(&mut self) {
        self.finish();
    }
}

pub async fn run(
    config: &Config,
    cloudflared: &std::path::Path,
    token: &str,
    server: &mut tokio::task::JoinHandle<Result<()>>,
) -> Result<()> {
    let address: std::net::SocketAddr = config
        .daemon_addr
        .parse()
        .map_err(|_| Error::msg("Invalid listener address"))?;
    if address.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
        return Err(Error::msg("Named tunnels require a 127.0.0.1 listener"));
    }
    let progress = TunnelProgress::new();
    progress.set("Reserving your tunnel…");
    let assignment = allocate(address.port()).await?;
    progress.set("Connecting to Cloudflare…");
    // Token never appears in process arguments or logs. File is deleted on exit.
    let mut token_file = tempfile::NamedTempFile::new_in(root()?)?;
    token_file.write_all(assignment.tunnel_token.as_bytes())?;
    token_file.as_file().sync_all()?;
    let mut child = Command::new(cloudflared)
        .args(["tunnel", "run", "--token-file"])
        .arg(token_file.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stderr = child.stderr.take().expect("piped connector output");
    let output = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        // Do not forward raw connector lines: upstream output may contain credentials.
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("ERR") {
                tracing::warn!("Named tunnel connector reported an error; checking connectivity");
            }
        }
    });
    let readiness = async {
        // Avoid caching NXDOMAIN while a newly created DNS record propagates.
        // The allocator persists this deadline, so reconnects do not wait again.
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let grace_ms = assignment
            .dns_ready_after
            .unwrap_or(0)
            .saturating_sub(now)
            .min(30_000);
        if grace_ms > 0 {
            progress.set("Waiting for DNS to propagate…");
            tokio::time::sleep(Duration::from_millis(grace_ms)).await;
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| Error::msg("Cannot initialize tunnel HTTP client"))?;
        progress.set("Checking secure connection…");
        let endpoint = format!("https://{}/web/session", assignment.hostname);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            // Validate the actual production browser path, with the daemon's token.
            let attempt = client
                .post(&endpoint)
                .header("origin", crate::rpc::web_auth::WEB_ORIGIN)
                .bearer_auth(token)
                .send()
                .await;
            let mut last_failure =
                String::from("invalid browser ticket response or CORS configuration");
            let response = match attempt {
                Ok(response) => {
                    if !response.status().is_success() {
                        last_failure = format!(
                            "HTTP {} from the public endpoint",
                            response.status().as_u16()
                        );
                    }
                    Some(response)
                }
                Err(error) => {
                    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
                    while let Some(cause) = source {
                        let message = cause.to_string().to_ascii_lowercase();
                        if message.contains("certificate")
                            || message.contains("handshakefailure")
                            || message.contains("handshake failure")
                        {
                            return Err(Error::msg(format!("TLS handshake failed for {}. Check Cloudflare edge certificate coverage for this hostname. Retrying cannot repair a missing certificate.", assignment.hostname)));
                        }
                        source = cause.source();
                    }
                    if error.is_timeout() {
                        last_failure = "public request timed out".into();
                    } else if error.is_connect() {
                        last_failure = "DNS lookup or public connection failed".into();
                    }
                    None
                }
            };
            if let Some(response) = response {
                if response.status().is_success()
                    && response
                        .headers()
                        .get("access-control-allow-origin")
                        .and_then(|v| v.to_str().ok())
                        == Some(crate::rpc::web_auth::WEB_ORIGIN)
                {
                    if let Ok(bytes) = response.bytes().await {
                        if serde_json::from_slice::<serde_json::Value>(&bytes)
                            .ok()
                            .is_some_and(|v| {
                                v["web_api_version"] == 1 && v["ticket"].as_str().is_some()
                            })
                        {
                            progress.finish();
                            println!("Remote URL: {}\nAccess token: {token}", assignment.url);
                            return Ok::<(), Error>(());
                        }
                    }
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::msg(format!("Named tunnel did not become reachable within 60 seconds: {last_failure}. The assignment is retained; check connector egress/DNS and retry.")));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    supervise_connector(child, output, readiness, server).await
}

async fn supervise_connector(
    mut child: tokio::process::Child,
    output: tokio::task::JoinHandle<()>,
    readiness: impl std::future::Future<Output = Result<()>>,
    server: &mut tokio::task::JoinHandle<Result<()>>,
) -> Result<()> {
    let shutdown = crate::rpc::server::shutdown_signal();
    tokio::pin!(shutdown);
    let startup = tokio::select! {
        result = readiness => result.map(|_| true),
        _ = &mut shutdown => Ok(false),
        result = &mut *server => result.unwrap_or_else(|_| Err(Error::msg("Daemon server stopped"))).map(|_| false),
        _ = child.wait() => Err(Error::msg("Named tunnel connector exited before becoming ready")),
    };
    // Only enter the steady-state wait if startup succeeded and shutdown wasn't requested.
    let outcome = if matches!(startup, Ok(true)) {
        tokio::select! {
            _ = &mut shutdown => Ok(()),
            result = &mut *server => result.unwrap_or_else(|_| Err(Error::msg("Daemon server stopped"))),
            _ = child.wait() => Err(Error::msg("Named tunnel connector exited; restart to reuse this installation's hostname")),
        }
    } else {
        startup.map(|_| ())
    };
    let _ = child.kill().await;
    let _ = child.wait().await;
    output.abort();
    let _ = output.await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn connector_exit_before_readiness_is_reported() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let output = tokio::spawn(std::future::pending());
        let mut server = tokio::spawn(std::future::pending());
        let result = supervise_connector(child, output, std::future::pending(), &mut server).await;
        server.abort();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("before becoming ready"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_readiness_kills_connector() {
        let child = Command::new("/bin/sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let output = tokio::spawn(std::future::pending());
        let mut server = tokio::spawn(std::future::pending());
        let result = supervise_connector(
            child,
            output,
            async { Err(Error::msg("not ready")) },
            &mut server,
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap_err().to_string(), "not ready");
        assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn server_failure_after_readiness_stops_connector() {
        let child = Command::new("/bin/sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let output = tokio::spawn(std::future::pending());
        let mut server = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Err(Error::msg("server failed"))
        });
        let result = supervise_connector(child, output, async { Ok(()) }, &mut server).await;
        assert_eq!(result.unwrap_err().to_string(), "server failed");
        assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_err());
    }

    #[test]
    fn identity_is_private_persistent_and_requires_explicit_creation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        assert!(identity_at(&path, false).is_err());
        let first = identity_at(&path, true).unwrap();
        let second = identity_at(&path, true).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.credential, second.credential);
        assert_eq!(first.credential.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(identity_at(&path, false).is_err());
        }
    }

    #[test]
    fn assignment_cannot_redirect_credentials_or_change_identity() {
        let identity = Identity {
            id: uuid::Uuid::new_v4().to_string(),
            credential: "a".repeat(64),
        };
        let hostname = format!("m-{}.example.com", identity.id);
        let mut assignment = Assignment {
            installation_id: identity.id.clone(),
            url: format!("wss://{hostname}/rpc"),
            hostname,
            port: 43721,
            dns_ready_after: None,
            tunnel_token: "connector-secret".into(),
        };
        assert!(validate_assignment(&assignment, &identity, 43721).is_ok());
        assignment.hostname = "a7c42e19.amarjay.com".into();
        assignment.url = format!("wss://{}/rpc", assignment.hostname);
        assert!(validate_assignment(&assignment, &identity, 43721).is_ok());
        for invalid in [
            "a7c42e19.evil.example",
            "a7c42e19.amarjay.com.evil.example",
            "a7c42e1.amarjay.com",
        ] {
            assignment.hostname = invalid.into();
            assignment.url = format!("wss://{invalid}/rpc");
            assert!(validate_assignment(&assignment, &identity, 43721).is_err());
        }
        assignment.hostname = "a7c42e19.amarjay.com".into();
        assignment.url = "wss://unrelated.example/rpc".into();
        assert!(validate_assignment(&assignment, &identity, 43721).is_err());
        assignment.url = format!("wss://{}/rpc?token=bad", assignment.hostname);
        assert!(validate_assignment(&assignment, &identity, 43721).is_err());
    }
}
