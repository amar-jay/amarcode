//! Foreground remote mode, with named Cloudflare access or private loopback access.
use crate::{App, Config, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
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

/// Updates Worker origin that allocates persistent named tunnels.
pub const REMOTE_ALLOCATOR_ORIGIN: &str = "https://updates.amarcode.amarjay.com";

#[derive(Default, Deserialize)]
pub struct Setup {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub listen_addr: Option<String>,
    #[serde(default)]
    pub cloudflared: Option<PathBuf>,
    #[serde(default)]
    pub tool_bin_dirs: Vec<PathBuf>,
}

pub fn instance_root() -> Result<PathBuf> {
    crate::config::resolve()?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| Error::msg("Cannot locate the daemon instance directory"))
}

/// Load optional access settings for the standard instance before Tokio starts.
/// Legacy explicit roots remain available for deliberately isolated instances.
pub fn configured() -> Result<Option<Setup>> {
    let root = instance_root()?;
    match fs::read(root.join("remote.json")) {
        Ok(bytes) => {
            let setup: Setup = serde_json::from_slice(&bytes)?;
            apply_environment(&root, &setup, false)?;
            if setup.enabled {
                Ok(Some(setup))
            } else {
                Ok(None)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Persist access settings without starting a second daemon or a tunnel.
pub fn configure_access(enabled: bool) -> Result<()> {
    let root = instance_root()?;
    fs::create_dir_all(&root)?;
    let path = root.join("remote.json");
    let mut settings: serde_json::Value = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e.into()),
    };
    let object = settings
        .as_object_mut()
        .ok_or_else(|| Error::msg("remote.json must be an object"))?;
    object.insert("enabled".into(), enabled.into());
    if enabled {
        let token = root.join("access-token");
        if !token.exists() {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&token)?;
            writeln!(
                file,
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )?;
            file.sync_all()?;
        }
        crate::rpc::websocket::load_token(&token)?;
    }
    crate::update::releases::atomic_write(&path, &serde_json::to_vec_pretty(&settings)?)
        .map_err(Error::msg)?;
    println!(
        "Remote access {}. Restart the existing daemon to apply.\nAccess token file: {}",
        if enabled { "enabled" } else { "disabled" },
        root.join("access-token").display()
    );
    Ok(())
}

/// Called before Tokio starts. Remote access shares the standard database.
pub fn prepare(require_tunnel: bool) -> Result<Setup> {
    if require_tunnel {
        crate::security::Policy::load()?.require_tunnel_allowed()?;
    }
    let root = remote_root()?;
    let setup: Setup = match fs::read(root.join("remote.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if !require_tunnel && error.kind() == std::io::ErrorKind::NotFound => Setup::default(),
        Err(_) => return Err(Error::msg("Remote setup is missing or unreadable; use an existing remote installation, or --remote --no-tunnel with a configured token file")),
    };
    if require_tunnel
        && setup
            .cloudflared
            .as_ref()
            .is_none_or(|path| !path.is_absolute() || !path.is_file())
    {
        return Err(Error::msg(
            "Installed cloudflared is missing; rerun the Amarcode installer",
        ));
    }
    apply_environment(&root, &setup, env::var_os("AMARCODE_REMOTE_ROOT").is_some())?;
    Ok(setup)
}

fn apply_environment(root: &std::path::Path, setup: &Setup, isolated: bool) -> Result<()> {
    if !setup.tool_bin_dirs.is_empty() {
        let path = env::var_os("PATH").unwrap_or_default();
        let mut directories = Vec::new();
        for directory in &setup.tool_bin_dirs {
            if !directory.is_absolute() || !directory.is_dir() {
                return Err(Error::msg(
                    "Installed agent runtimes are missing; rerun the Amarcode installer",
                ));
            }
            if !directories.contains(directory) {
                directories.push(directory.clone());
            }
        }
        for directory in env::split_paths(&path) {
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
        env::set_var(
            "PATH",
            env::join_paths(directories)
                .map_err(|error| Error::msg(format!("Invalid installed runtime PATH: {error}")))?,
        );
    }
    if isolated && env::var_os("AMARCODE_APPDIR").is_none() {
        env::set_var("AMARCODE_APPDIR", root.join("data"));
    }
    if env::var_os("AMARCODE_WS_TOKEN_FILE").is_none() {
        env::set_var("AMARCODE_WS_TOKEN_FILE", root.join("access-token"));
    }
    Ok(())
}

/// Access configuration cloned from the single application's configuration.
pub fn access_config(local: &Config, setup: &Setup) -> Result<Config> {
    let mut remote = local.clone();
    remote.transport = crate::config::DaemonTransport::WebSocket;
    remote.daemon_addr = env::var("AMARCODE_WS_ADDR")
        .ok()
        .or_else(|| setup.listen_addr.clone())
        .unwrap_or_else(|| crate::config::DEFAULT_WS_DAEMON_ADDR.into());
    remote.ws_token_file = env::var_os("AMARCODE_WS_TOKEN_FILE")
        .map(PathBuf::from)
        .or(Some(instance_root()?.join("access-token")));
    remote.ws_tls_cert = env::var_os("AMARCODE_WS_TLS_CERT").map(PathBuf::from);
    remote.ws_tls_key = env::var_os("AMARCODE_WS_TLS_KEY").map(PathBuf::from);
    remote.ws_client_ca = env::var_os("AMARCODE_WS_CLIENT_CA").map(PathBuf::from);
    remote.ws_allowed_origins = env::var("AMARCODE_WS_ALLOW_ORIGIN")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    remote.validate()?;
    Ok(remote)
}

pub async fn run_shared(local: Config, setup: Setup, no_tunnel: bool) -> Result<()> {
    let remote = access_config(&local, &setup)?;
    validate(&remote, no_tunnel)?;
    let listener = crate::rpc::websocket::Listener::bind(&remote).await?;
    let app = App::new(local).await?;
    let (ready, listening) = tokio::sync::oneshot::channel();
    let stop = app.shutdown_handle();
    let server = tokio::spawn(app.run_with_access(listener, ready));
    if listening.await.is_err() {
        return server.await.map_err(|e| Error::msg(e.to_string()))?;
    }
    finish(remote, setup, no_tunnel, server, Some(stop)).await
}

/// Attach Cloudflare to a live authenticated listener; never opens a database.
pub async fn tunnel(local: Config, setup: Setup) -> Result<()> {
    let remote = access_config(&local, &setup)?;
    validate(&remote, false)?;
    check_health(&remote).await?;
    finish(
        remote,
        setup,
        false,
        tokio::spawn(std::future::pending()),
        None,
    )
    .await
}

async fn check_health(remote: &Config) -> Result<()> {
    check_health_with_origin(remote, origin_tls(remote)?).await
}

async fn check_health_with_origin(remote: &Config, origin: Option<OriginTls>) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    let token = crate::rpc::websocket::load_token(
        remote
            .ws_token_file
            .as_deref()
            .ok_or_else(|| Error::msg("Remote token file missing"))?,
    )?;
    let host = origin.as_ref().map(|tls| tls.server_name.as_str());
    let port = remote
        .daemon_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|e| Error::msg(e.to_string()))?
        .port();
    let url = match host {
        Some(host) => format!("wss://{host}:{port}/rpc"),
        None => format!("ws://{}/rpc", remote.daemon_addr),
    };
    let mut request = url
        .into_client_request()
        .map_err(|e| Error::msg(e.to_string()))?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| Error::msg("Invalid remote token"))?,
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let tcp = tokio::net::TcpStream::connect(&remote.daemon_addr).await?;
        let ca = origin
            .as_ref()
            .and_then(|tls| tls.ca_pool.as_deref())
            .map(std::path::Path::new);
        let tls = amarcode_transport::client_config_with(ca, None, None).map_err(Error::msg)?;
        let connector = tokio_tungstenite::Connector::Rustls(Arc::new(tls));
        let (mut socket, _) =
            tokio_tungstenite::client_async_tls_with_config(request, tcp, None, Some(connector))
                .await
                .map_err(|e| {
                    Error::msg(format!(
                        "Start or restart the existing daemon before opening a tunnel: {e}"
                    ))
                })?;
        socket
            .send(Message::Text(
                serde_json::json!({"method":"health"}).to_string().into(),
            ))
            .await
            .map_err(|e| Error::msg(e.to_string()))?;
        let response = socket
            .next()
            .await
            .ok_or_else(|| Error::msg("Daemon closed the health connection"))?
            .map_err(|e| Error::msg(e.to_string()))?;
        let reply: serde_json::Value =
            serde_json::from_str(response.to_text().map_err(|e| Error::msg(e.to_string()))?)?;
        if reply["result"]["remote_auth_required"] != true || reply["result"]["status"] != "ok" {
            return Err(Error::msg(
                "Listener is not an authenticated Amarcode daemon",
            ));
        }
        if reply["result"]["protocol_version"] != amarcode_protocol::PROTOCOL_VERSION {
            return Err(Error::msg(
                "The running daemon has an incompatible protocol version",
            ));
        }
        socket
            .close(None)
            .await
            .map_err(|e| Error::msg(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|_| Error::msg("Daemon health check timed out"))??;
    Ok(())
}

fn announce_loopback(config: &Config) -> Result<()> {
    let token_path = config
        .ws_token_file
        .as_deref()
        .ok_or_else(|| Error::msg("Remote access token is not configured"))?;
    println!(
        "Authenticated daemon listening on {}\nAccess token file: {}",
        config.daemon_addr,
        token_path.display()
    );
    Ok(())
}

pub fn validate(config: &Config, no_tunnel: bool) -> Result<()> {
    config.validate()?;
    if !no_tunnel {
        crate::security::Policy::load()?.require_tunnel_allowed()?;
        if config.ws_client_ca.is_some() {
            return Err(Error::msg(
                "Managed tunnels do not support client certificates; use direct mTLS",
            ));
        }
    }
    let address: std::net::SocketAddr = config
        .daemon_addr
        .parse()
        .map_err(|_| Error::msg("Invalid remote daemon address"))?;
    if config.transport != crate::config::DaemonTransport::WebSocket
        || !address.ip().is_loopback()
        || address.port() == 0
    {
        return Err(Error::msg(
            "Remote mode requires a loopback WebSocket listener with a fixed port",
        ));
    }
    if no_tunnel && address.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
        return Err(Error::msg(
            "--remote --no-tunnel requires 127.0.0.1 for private loopback access",
        ));
    }
    Ok(())
}

pub async fn run(config: Config, setup: Setup, no_tunnel: bool) -> Result<()> {
    validate(&config, no_tunnel)?;
    crate::rpc::websocket::load_token(
        config
            .ws_token_file
            .as_deref()
            .ok_or_else(|| Error::msg("Remote access token file is not configured"))?,
    )?;
    let app = App::new(config.clone()).await?;
    let (ready, listening) = tokio::sync::oneshot::channel();
    let stop = app.shutdown_handle();
    let server = tokio::spawn(app.run_with_ready(ready));
    if listening.await.is_err() {
        return server.await.map_err(|e| Error::msg(e.to_string()))?;
    }
    finish(config, setup, no_tunnel, server, Some(stop)).await
}

async fn finish(
    config: Config,
    setup: Setup,
    no_tunnel: bool,
    mut server: tokio::task::JoinHandle<Result<()>>,
    stop: Option<crate::shutdown::Shutdown>,
) -> Result<()> {
    // Every exit, including setup/token errors, must stop the server we own.
    let result = async {
        let cloudflared = if no_tunnel {
            None
        } else {
            Some(
                setup
                    .cloudflared
                    .as_deref()
                    .ok_or_else(|| Error::msg("Installed cloudflared is missing"))?,
            )
        };
        let token_path = config
            .ws_token_file
            .as_deref()
            .ok_or_else(|| Error::msg("Remote access token file is not configured"))?;
        let token = crate::rpc::websocket::load_token(token_path)?;
        let Some(cloudflared) = cloudflared else {
            announce_loopback(&config)?;
            return (&mut server)
                .await
                .map_err(|error| Error::msg(error.to_string()))?;
        };
        if let Ok(mode) = std::env::var("AMARCODE_TUNNEL_MODE") {
            if mode != "named" {
                return Err(Error::msg(
                    "Only named Cloudflare tunnels are supported; unset AMARCODE_TUNNEL_MODE",
                ));
            }
        }
        run_named(&config, cloudflared, &token, &mut server).await
    }
    .await;
    stop_server(&mut server, stop, Duration::from_secs(12)).await;
    result
}

async fn stop_server(
    server: &mut tokio::task::JoinHandle<Result<()>>,
    stop: Option<crate::shutdown::Shutdown>,
    deadline: Duration,
) {
    if let Some(stop) = stop {
        stop.cancel();
        // The connector supervisor may already have consumed the server result.
        if server.is_finished() {
            return;
        }
        // Allow both the five-second RPC drain and five-second worker budget.
        if tokio::time::timeout(deadline, &mut *server).await.is_ok() {
            return;
        }
        tracing::warn!("Daemon exceeded tunnel shutdown deadline; aborting server task");
    }
    // Attach-only mode owns this placeholder task, never the live daemon.
    server.abort();
    if tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .is_err()
    {
        tracing::warn!("Daemon server task did not stop after abort");
    }
}

// Persistent named-tunnel enrollment and connector lifecycle.
#[derive(Serialize, Deserialize)]
struct Identity {
    id: String,
    credential: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct OriginTls {
    server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ca_pool: Option<String>,
}
fn origin_tls(config: &Config) -> Result<Option<OriginTls>> {
    if config.ws_tls_cert.is_none() {
        return Ok(None);
    }
    let server_name =
        env::var("AMARCODE_TUNNEL_ORIGIN_SERVER_NAME").unwrap_or_else(|_| "localhost".into());
    let ca_pool = env::var_os("AMARCODE_TUNNEL_ORIGIN_CA").map(PathBuf::from);
    if let Some(path) = &ca_pool {
        if !path.is_absolute() || !path.is_file() {
            return Err(Error::msg(
                "AMARCODE_TUNNEL_ORIGIN_CA must name an existing absolute PEM path",
            ));
        }
        amarcode_transport::certificates(path).map_err(Error::msg)?;
    }
    Ok(Some(OriginTls {
        server_name,
        ca_pool: ca_pool.map(|p| p.to_string_lossy().into_owned()),
    }))
}
#[derive(Deserialize)]
struct Assignment {
    installation_id: String,
    hostname: String,
    url: String,
    tunnel_token: String,
    port: u16,
    #[serde(default)]
    origin_tls: Option<OriginTls>,
    #[serde(default)]
    dns_ready_after: Option<u64>,
}
fn remote_root() -> Result<PathBuf> {
    std::env::var_os("AMARCODE_REMOTE_ROOT")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(instance_root)
}
fn identity_path() -> Result<PathBuf> {
    Ok(remote_root()?.join("named-installation.json"))
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
    enrollment: Option<(u16, Option<OriginTls>)>,
) -> Result<reqwest::Response> {
    let mut request = client()?
        .request(
            method,
            format!("{REMOTE_ALLOCATOR_ORIGIN}/v1/tunnels/{}", identity.id),
        )
        .bearer_auth(&identity.credential)
        .header("x-amarcode-tunnel-hostnames", "short-v2");
    if let Some((port, origin_tls)) = enrollment {
        let mut body = serde_json::json!({"port": port});
        if let Some(tls) = origin_tls {
            body["origin_tls"] = serde_json::to_value(tls)?;
        }
        request = request
            .body(body.to_string())
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
async fn allocate(port: u16, origin: Option<OriginTls>) -> Result<Assignment> {
    let identity = identity(true)?;
    let assignment: Assignment = serde_json::from_slice(
        &request(
            reqwest::Method::PUT,
            &identity,
            Some((port, origin.clone())),
        )
        .await?
        .bytes()
        .await
        .map_err(|_| Error::msg("Cannot read tunnel allocation response"))?,
    )?;
    validate_assignment(&assignment, &identity, port)?;
    if assignment.origin_tls != origin {
        return Err(Error::msg(
            "Allocator did not confirm the requested origin TLS settings; refusing tunnel startup",
        ));
    }
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

/// Permanently revoke this installation's named tunnel and reset local enrollment.
pub async fn remove_named() -> Result<()> {
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

async fn run_named(
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
    crate::security::Policy::load()?.require_tunnel_allowed()?;
    let progress = TunnelProgress::new();
    progress.set("Reserving your tunnel…");
    let assignment = allocate(address.port(), origin_tls(config)?).await?;
    progress.set("Connecting to Cloudflare…");
    // Token never appears in process arguments or logs. File is deleted on exit.
    let mut token_file = tempfile::NamedTempFile::new_in(remote_root()?)?;
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

    #[tokio::test]
    async fn tunnel_setup_failure_gracefully_stops_owned_single_and_shared_servers() {
        for shared in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let config = Config {
                app_dir: root.path().to_owned(),
                db_path: root.path().join("workspace.sqlite3"),
                daemon_addr: "127.0.0.1:0".into(),
                transport: Default::default(),
                default_workspace_path: None,
                acp_registry_source: None,
                ws_token_file: None,
                ws_tls_cert: None,
                ws_tls_key: None,
                ws_client_ca: None,
                ws_allowed_origins: vec![],
            };
            let app = App::new(config.clone()).await.unwrap();
            let stop = app.shutdown_handle();
            let queue = Arc::clone(&app.prompt_queue);
            let (ready, listening) = tokio::sync::oneshot::channel();
            let server = if shared {
                let path = root.path().join("token");
                fs::write(&path, "a".repeat(64)).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                }
                let mut remote = config.clone();
                remote.transport = crate::config::DaemonTransport::WebSocket;
                remote.ws_token_file = Some(path);
                let access = crate::rpc::websocket::Listener::bind(&remote)
                    .await
                    .unwrap();
                tokio::spawn(app.run_with_access(access, ready))
            } else {
                tokio::spawn(app.run_with_ready(ready))
            };
            listening.await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                finish(config.clone(), Setup::default(), false, server, Some(stop)),
            )
            .await
            .expect("tunnel failure must finish daemon cleanup");
            assert_eq!(
                result.unwrap_err().to_string(),
                "Installed cloudflared is missing"
            );
            let error = queue
                .update(crate::protocol::rpc::UpdatePromptQueueParams {
                    chat_id: "unused".into(),
                    action: "pause".into(),
                    id: None,
                    text: None,
                })
                .unwrap_err();
            assert!(error.to_string().contains("shutting down"), "{error}");
            // The server has released instance ownership, rather than leaving a
            // detached task holding the database after an early setup error.
            let next = App::new(config).await.unwrap();
            next.request_shutdown();
        }
    }

    #[tokio::test]
    async fn tunnel_shutdown_waits_for_cleanup_and_bounds_unresponsive_servers() {
        let stop = crate::shutdown::Shutdown::default();
        let signal = stop.clone();
        let (cleaned, cleanup) = tokio::sync::oneshot::channel();
        let mut server = tokio::spawn(async move {
            signal.cancelled().await;
            tokio::time::sleep(Duration::from_millis(30)).await;
            cleaned.send(()).unwrap();
            Ok(())
        });
        stop_server(&mut server, Some(stop), Duration::from_secs(1)).await;
        cleanup
            .await
            .expect("cleanup completed before shutdown returned");

        let mut stuck = tokio::spawn(std::future::pending());
        tokio::time::timeout(
            Duration::from_secs(1),
            stop_server(
                &mut stuck,
                Some(crate::shutdown::Shutdown::default()),
                Duration::from_millis(20),
            ),
        )
        .await
        .expect("fallback must be bounded");
        assert!(stuck.is_finished());
    }

    #[tokio::test]
    async fn origin_health_checks_verify_tls_with_a_private_ca() {
        let root = tempfile::tempdir().unwrap();
        let certs = root.path().join("tls");
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/tls-certificates.py");
        assert!(std::process::Command::new("python3")
            .arg(script)
            .args(["dev-ca", "--hostname", "localhost", "--output"])
            .arg(&certs)
            .output()
            .unwrap()
            .status
            .success());
        let token = root.path().join("token");
        fs::write(&token, "a".repeat(64)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let local = Config {
            app_dir: root.path().join("data"),
            db_path: root.path().join("data/store.sqlite3"),
            transport: Default::default(),
            daemon_addr: "127.0.0.1:0".into(),
            default_workspace_path: None,
            acp_registry_source: None,
            ws_token_file: None,
            ws_tls_cert: None,
            ws_tls_key: None,
            ws_client_ca: None,
            ws_allowed_origins: vec![],
        };
        let mut remote = local.clone();
        remote.transport = crate::config::DaemonTransport::WebSocket;
        remote.ws_token_file = Some(token);
        remote.ws_tls_cert = Some(certs.join("server-cert.pem"));
        remote.ws_tls_key = Some(certs.join("server-key.pem"));
        let listener = crate::rpc::websocket::Listener::bind(&remote)
            .await
            .unwrap();
        remote.daemon_addr = listener.local_addr().unwrap().to_string();
        let app = App::new(local).await.unwrap();
        let stop = app.shutdown_handle();
        let (ready, listening) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(app.run_with_access(listener, ready));
        listening.await.unwrap();
        let origin = OriginTls {
            server_name: "localhost".into(),
            ca_pool: Some(certs.join("ca.pem").to_string_lossy().into_owned()),
        };
        assert!(check_health_with_origin(&remote, Some(origin.clone()))
            .await
            .is_ok());
        assert!(check_health_with_origin(
            &remote,
            Some(OriginTls {
                server_name: "wrong.example".into(),
                ..origin
            })
        )
        .await
        .is_err());
        assert!(check_health_with_origin(
            &remote,
            Some(OriginTls {
                server_name: "localhost".into(),
                ca_pool: None
            })
        )
        .await
        .is_err());
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
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
            origin_tls: None,
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
