//! Session-scoped, cookie-authenticated X11 streams multiplexed over a terminal.
//! Each stream has a bounded credit window, independent of terminal input.
use crate::{Error, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::path::PathBuf;
use std::{
    collections::HashMap,
    io::Write,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Semaphore},
    task::{AbortHandle, JoinSet},
};

const BLOCK: usize = 16 * 1024;
const WINDOW: usize = 8;
pub(crate) const TERMINAL_WINDOW: usize = 8;
const MAX_CONNECTIONS: usize = 32;
const AUTH_NAME: &[u8] = b"MIT-MAGIC-COOKIE-1";
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize, Serialize)]
pub(crate) struct Request {
    pub version: u8,
    pub cookie: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
pub(crate) enum Frame {
    #[serde(rename = "x11_open")]
    Open { id: u32 },
    #[serde(rename = "x11_data")]
    Data { id: u32, data: String },
    #[serde(rename = "x11_ack")]
    Ack { id: u32 },
    #[serde(rename = "x11_close")]
    Close { id: u32 },
}

fn cookie(value: &str) -> Result<[u8; 16]> {
    hex::decode(value)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| Error::msg("X11 requires a 128-bit MIT-MAGIC-COOKIE-1 cookie"))
}

/// A private authority file and an IPv4-loopback listener owned by one shell.
pub(crate) struct RemoteDisplay {
    pub listener: TcpListener,
    pub display: String,
    pub authority: tempfile::NamedTempFile,
}
impl RemoteDisplay {
    pub async fn new(request: &Request) -> Result<Self> {
        if request.version != 1 {
            return Err(Error::msg("Unsupported X11 forwarding version"));
        }
        let cookie = cookie(&request.cookie)?;
        for number in 10..1010 {
            match TcpListener::bind(("127.0.0.1", 6000 + number)).await {
                Ok(listener) => {
                    let mut authority = tempfile::NamedTempFile::new()?;
                    authority.write_all(&authority_record(&number.to_string(), &cookie))?;
                    authority.flush()?;
                    return Ok(Self {
                        listener,
                        authority,
                        display: format!("127.0.0.1:{number}.0"),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(Error::msg("No available loopback X11 display"))
    }
}

fn authority_record(display: &str, cookie: &[u8; 16]) -> Vec<u8> {
    // Xauthority integers use network byte order. FamilyWild matches both
    // IPv4-loopback and Xlib's hostname lookup; the display number still matches.
    let mut record = 65535u16.to_be_bytes().to_vec();
    for field in [b"".as_slice(), display.as_bytes(), AUTH_NAME, cookie] {
        record.extend_from_slice(&(field.len() as u16).to_be_bytes());
        record.extend_from_slice(field);
    }
    record
}

#[derive(Clone)]
enum Address {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Unix(PathBuf),
}
#[derive(Clone)]
pub(crate) struct LocalDisplay {
    address: Address,
    real_cookie: [u8; 16],
    fake_cookie: [u8; 16],
}
impl LocalDisplay {
    pub async fn from_environment() -> Result<Self> {
        let display = std::env::var("DISPLAY")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                Error::msg("X11 forwarding needs a running local X server and DISPLAY")
            })?;
        let address = display_address(&display)?;
        let mut command = tokio::process::Command::new("xauth");
        command.args(["list", &display]).kill_on_drop(true);
        let output = tokio::time::timeout(TIMEOUT, command.output())
            .await
            .map_err(|_| Error::msg("Local xauth lookup timed out"))?
            .map_err(|_| Error::msg("Install xauth locally to enable X11 forwarding"))?;
        if !output.status.success() {
            return Err(Error::msg(
                "Could not read local X11 authentication with xauth",
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let real_cookie = text
            .lines()
            .find_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                (fields.len() == 3 && fields[1].as_bytes() == AUTH_NAME)
                    .then(|| cookie(fields[2]).ok())
                    .flatten()
            })
            .ok_or_else(|| {
                Error::msg("No MIT-MAGIC-COOKIE-1 entry for local DISPLAY; check XAUTHORITY")
            })?;
        Ok(Self {
            address,
            real_cookie,
            fake_cookie: *uuid::Uuid::new_v4().as_bytes(),
        })
    }
    pub fn request(&self) -> Request {
        Request {
            version: 1,
            cookie: hex::encode(self.fake_cookie),
        }
    }
    async fn connect(&self) -> Result<Box<dyn Io>> {
        match &self.address {
            Address::Tcp(address) => Ok(Box::new(TcpStream::connect(address).await?)),
            #[cfg(unix)]
            Address::Unix(path) => Ok(Box::new(tokio::net::UnixStream::connect(path).await?)),
        }
    }
}

fn display_address(display: &str) -> Result<Address> {
    let (host, suffix) = display
        .rsplit_once(':')
        .ok_or_else(|| Error::msg("Invalid local DISPLAY"))?;
    let mut parts = suffix.split('.');
    let number: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n <= 59535)
        .ok_or_else(|| Error::msg("Invalid X11 display number"))?;
    if let Some(screen) = parts.next() {
        if screen.parse::<u16>().is_err() || parts.next().is_some() {
            return Err(Error::msg("Invalid X11 screen number"));
        }
    }
    #[cfg(unix)]
    if host.is_empty() || host == "unix" || host.ends_with("/unix") {
        return Ok(Address::Unix(PathBuf::from(format!(
            "/tmp/.X11-unix/X{number}"
        ))));
    }
    #[cfg(unix)]
    if host.starts_with('/') {
        // XQuartz launchd DISPLAY names are local Unix socket paths.
        return Ok(Address::Unix(PathBuf::from(display)));
    }
    // Windows X servers listen on loopback TCP for `:N` and `unix:N`.
    #[cfg(not(unix))]
    let host = if host.is_empty() || host == "unix" {
        "localhost"
    } else {
        host
    };
    let ip = match host {
        "localhost" => IpAddr::from([127, 0, 0, 1]),
        "[::1]" | "::1" => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        _ => host.parse().map_err(|_| {
            Error::msg("Use a local Unix or loopback TCP DISPLAY for X11 forwarding")
        })?,
    };
    if !ip.is_loopback() {
        return Err(Error::msg(
            "X11 forwarding only connects to a local display",
        ));
    }
    Ok(Address::Tcp(SocketAddr::new(ip, 6000 + number)))
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

struct Peer {
    input: mpsc::Sender<Vec<u8>>,
    credits: Arc<Semaphore>,
    outstanding: Arc<AtomicUsize>,
    task: AbortHandle,
}
pub(crate) struct Mux {
    peers: HashMap<u32, Peer>,
    tasks: JoinSet<()>,
    events: mpsc::Receiver<Frame>,
    sender: mpsc::Sender<Frame>,
    last_id: u32,
}
impl Mux {
    pub fn new() -> Self {
        let (sender, events) = mpsc::channel(64);
        Self {
            peers: HashMap::new(),
            tasks: JoinSet::new(),
            events,
            sender,
            last_id: 0,
        }
    }
    pub fn can_open(&self) -> bool {
        self.peers.len() < MAX_CONNECTIONS
    }
    pub fn next_id(&self) -> Result<u32> {
        self.last_id
            .checked_add(1)
            .ok_or_else(|| Error::msg("X11 channel ID exhausted"))
    }
    pub fn open_remote(&mut self, id: u32, stream: TcpStream) -> Result<()> {
        self.open(id, Some(Box::new(stream)), None)
    }
    pub fn open_local(&mut self, id: u32, local: LocalDisplay) -> Result<()> {
        self.open(id, None, Some(local))
    }
    fn open(
        &mut self,
        id: u32,
        stream: Option<Box<dyn Io>>,
        local: Option<LocalDisplay>,
    ) -> Result<()> {
        if id <= self.last_id || !self.can_open() {
            return Err(Error::msg("Invalid or excessive X11 channels"));
        }
        self.last_id = id;
        while self.tasks.try_join_next().is_some() {}
        let (input, mut receiver) = mpsc::channel::<Vec<u8>>(WINDOW);
        let credits = Arc::new(Semaphore::new(WINDOW));
        let outstanding = Arc::new(AtomicUsize::new(0));
        let (read_credits, read_outstanding) = (credits.clone(), outstanding.clone());
        let events = self.sender.clone();
        let task = self.tasks.spawn(async move {
            let result: Result<()> = async {
                let stream = match (stream, local) {
                    (Some(stream), _) => stream,
                    (_, Some(local)) => tokio::time::timeout(TIMEOUT, async {
                        let mut initial = Vec::new();
                        loop {
                            let bytes = receiver
                                .recv()
                                .await
                                .ok_or_else(|| Error::msg("X11 connection closed during setup"))?;
                            initial.extend_from_slice(&bytes);
                            events
                                .send(Frame::Ack { id })
                                .await
                                .map_err(|_| Error::msg("X11 session closed"))?;
                            if rewrite_setup(&mut initial, &local.fake_cookie, &local.real_cookie)?
                            {
                                break;
                            }
                        }
                        let mut stream = local.connect().await?;
                        stream.write_all(&initial).await?;
                        Ok::<_, Error>(stream)
                    })
                    .await
                    .map_err(|_| Error::msg("X11 connection setup timed out"))??,
                    _ => unreachable!(),
                };
                bridge(
                    id,
                    stream,
                    receiver,
                    events.clone(),
                    read_credits,
                    read_outstanding,
                )
                .await
            }
            .await;
            if let Err(error) = result {
                tracing::debug!(%error, channel = id, "X11 channel closed");
            }
            let _ = events.send(Frame::Close { id }).await;
        });
        self.peers.insert(
            id,
            Peer {
                input,
                credits,
                outstanding,
                task,
            },
        );
        Ok(())
    }
    pub fn receive(&mut self, frame: Frame) -> Result<()> {
        match frame {
            Frame::Data { id, data } => {
                if data.len() > BLOCK.div_ceil(3) * 4 {
                    return Err(Error::msg("X11 frame too large"));
                }
                let bytes = BASE64
                    .decode(data)
                    .map_err(|_| Error::msg("Invalid X11 data"))?;
                if bytes.is_empty() || bytes.len() > BLOCK {
                    return Err(Error::msg("Invalid X11 data size"));
                }
                if let Some(peer) = self.peers.get(&id) {
                    peer.input
                        .try_send(bytes)
                        .map_err(|_| Error::msg("X11 receive window exceeded"))?;
                }
            }
            Frame::Ack { id } => {
                if let Some(peer) = self.peers.get(&id) {
                    peer.outstanding
                        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                        .map_err(|_| Error::msg("Unexpected X11 acknowledgement"))?;
                    peer.credits.add_permits(1);
                }
            }
            Frame::Close { id } => self.close(id),
            Frame::Open { .. } => return Err(Error::msg("Unexpected X11 channel open")),
        }
        Ok(())
    }
    fn close(&mut self, id: u32) {
        if let Some(peer) = self.peers.remove(&id) {
            peer.task.abort();
        }
    }
    pub async fn next(&mut self) -> Option<Frame> {
        let frame = self.events.recv().await?;
        if let Frame::Close { id } = frame {
            self.close(id);
        }
        Some(frame)
    }
}

async fn bridge(
    id: u32,
    stream: Box<dyn Io>,
    mut input: mpsc::Receiver<Vec<u8>>,
    events: mpsc::Sender<Frame>,
    credits: Arc<Semaphore>,
    outstanding: Arc<AtomicUsize>,
) -> Result<()> {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let reads = async {
        let mut buffer = [0u8; BLOCK];
        loop {
            let permit = credits
                .acquire()
                .await
                .map_err(|_| Error::msg("X11 channel closed"))?;
            let count = reader.read(&mut buffer).await?;
            if count == 0 {
                drop(permit);
                // Do not let a close overtake bytes still queued at the peer.
                let _drained = credits
                    .acquire_many(WINDOW as u32)
                    .await
                    .map_err(|_| Error::msg("X11 channel closed"))?;
                return Ok::<_, Error>(());
            }
            outstanding.fetch_add(1, Ordering::SeqCst);
            permit.forget();
            events
                .send(Frame::Data {
                    id,
                    data: BASE64.encode(&buffer[..count]),
                })
                .await
                .map_err(|_| Error::msg("X11 session closed"))?;
        }
    };
    let writes = async {
        while let Some(bytes) = input.recv().await {
            writer.write_all(&bytes).await?;
            events
                .send(Frame::Ack { id })
                .await
                .map_err(|_| Error::msg("X11 session closed"))?;
        }
        Ok::<_, Error>(())
    };
    tokio::select! { result = reads => result, result = writes => result }
}

/// Buffer only the bounded connection prefix, validate its spoofed credential,
/// then substitute the real local cookie before any bytes reach the local server.
fn rewrite_setup(bytes: &mut [u8], fake: &[u8; 16], real: &[u8; 16]) -> Result<bool> {
    if bytes.len() < 12 {
        return Ok(false);
    }
    let word = |at| match bytes[0] {
        b'l' => Ok(u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize),
        b'B' => Ok(u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize),
        _ => Err(Error::msg("Invalid X11 byte order")),
    };
    let name = word(6)?;
    let data = word(8)?;
    if word(2)? != 11 || name != AUTH_NAME.len() || data != fake.len() {
        return Err(Error::msg(
            "X11 connection requires MIT-MAGIC-COOKIE-1 authentication",
        ));
    }
    let start = 12 + name.div_ceil(4) * 4;
    let end = start + data.div_ceil(4) * 4;
    if bytes.len() < end {
        return Ok(false);
    }
    let mismatch = bytes[start..start + data]
        .iter()
        .zip(fake)
        .fold(0, |acc, (a, b)| acc | (a ^ b));
    if &bytes[12..12 + name] != AUTH_NAME || mismatch != 0 {
        return Err(Error::msg("Invalid X11 forwarding cookie"));
    }
    bytes[start..start + data].copy_from_slice(real);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(fake: &[u8; 16], little: bool) -> Vec<u8> {
        let mut bytes = vec![0; 12];
        bytes[0] = if little { b'l' } else { b'B' };
        for (at, word) in [(2, 11u16), (6, AUTH_NAME.len() as u16), (8, 16)] {
            bytes[at..at + 2].copy_from_slice(&if little {
                word.to_le_bytes()
            } else {
                word.to_be_bytes()
            });
        }
        bytes.extend_from_slice(AUTH_NAME);
        bytes.resize(12 + AUTH_NAME.len().div_ceil(4) * 4, 0);
        bytes.extend_from_slice(fake);
        bytes
    }

    #[test]
    fn validates_fragmented_both_endian_handshakes_before_replacing_the_cookie() {
        for little in [true, false] {
            let original = setup(&[1; 16], little);
            for length in 0..original.len() {
                let mut prefix = original[..length].to_vec();
                assert!(!rewrite_setup(&mut prefix, &[1; 16], &[2; 16]).unwrap());
                assert_eq!(prefix, original[..length]);
            }
            let mut packet = original.clone();
            packet.extend_from_slice(b"subsequent X11 requests");
            assert!(rewrite_setup(&mut packet, &[1; 16], &[2; 16]).unwrap());
            assert_eq!(&packet[original.len() - 16..original.len()], &[2; 16]);
            assert_eq!(&packet[original.len()..], b"subsequent X11 requests");
            for mutation in [0, 2, 6, 8, 12, original.len() - 1] {
                let mut bad = original.clone();
                bad[mutation] ^= 1;
                assert!(rewrite_setup(&mut bad, &[1; 16], &[2; 16]).is_err());
            }
        }
    }

    #[test]
    fn only_local_display_targets_and_bounded_numbers_are_accepted() {
        for display in [
            ":0",
            "unix:1.2",
            "localhost:10",
            "127.0.0.1:12.0",
            "[::1]:3",
        ] {
            assert!(display_address(display).is_ok(), "{display}");
        }
        for display in [
            "example.com:0",
            "192.0.2.1:0",
            ":60000",
            ":-1",
            ":1.bad",
            ":1.2.3",
            "bad",
        ] {
            assert!(display_address(display).is_err(), "{display}");
        }
        assert!(cookie("short").is_err());
    }

    #[tokio::test]
    async fn remote_listener_and_private_authority_are_removed_with_the_session() {
        let remote = RemoteDisplay::new(&Request {
            version: 1,
            cookie: hex::encode([3; 16]),
        })
        .await
        .unwrap();
        let address = remote.listener.local_addr().unwrap();
        assert!(address.ip().is_loopback());
        let path = remote.authority.path().to_owned();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            authority_record(
                remote
                    .display
                    .split(':')
                    .nth(1)
                    .unwrap()
                    .split('.')
                    .next()
                    .unwrap(),
                &[3; 16]
            )
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        drop(remote);
        assert!(!path.exists());
        assert!(TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn invalid_forwarding_cookie_never_connects_to_the_local_x_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = LocalDisplay {
            address: Address::Tcp(listener.local_addr().unwrap()),
            real_cookie: [2; 16],
            fake_cookie: [1; 16],
        };
        let mut mux = Mux::new();
        mux.open_local(1, local).unwrap();
        mux.receive(Frame::Data {
            id: 1,
            data: BASE64.encode(setup(&[3; 16], true)),
        })
        .unwrap();
        tokio::time::timeout(TIMEOUT, async {
            while !matches!(mux.next().await.unwrap(), Frame::Close { id: 1 }) {}
        })
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn limits_channels_payloads_and_unsolicited_credits() {
        let mut mux = Mux::new();
        let mut keep = Vec::new();
        for id in 1..=MAX_CONNECTIONS as u32 {
            let (app, io) = tokio::io::duplex(16);
            keep.push(app);
            mux.open(id, Some(Box::new(io)), None).unwrap();
        }
        assert!(!mux.can_open());
        let (_, io) = tokio::io::duplex(16);
        assert!(mux.open(33, Some(Box::new(io)), None).is_err());
        assert!(mux.receive(Frame::Ack { id: 1 }).is_err());
        assert!(mux
            .receive(Frame::Data {
                id: 1,
                data: BASE64.encode(vec![0; BLOCK + 1])
            })
            .is_err());
        assert!(mux
            .receive(Frame::Data {
                id: 1,
                data: "invalid!".into()
            })
            .is_err());
        drop(mux);
        for mut app in keep {
            let mut byte = [0];
            assert_eq!(
                tokio::time::timeout(TIMEOUT, app.read(&mut byte))
                    .await
                    .unwrap()
                    .unwrap(),
                0
            );
        }
    }

    #[tokio::test]
    async fn large_bidirectional_streams_are_not_truncated_or_deadlocked_by_backpressure() {
        let mut left = Mux::new();
        let mut right = Mux::new();
        let (app_left, io_left) = tokio::io::duplex(BLOCK);
        let (app_right, io_right) = tokio::io::duplex(BLOCK);
        left.open(1, Some(Box::new(io_left)), None).unwrap();
        right.open(1, Some(Box::new(io_right)), None).unwrap();
        async fn exchange(
            io: tokio::io::DuplexStream,
            send: u8,
            receive: u8,
            sent: usize,
            received: usize,
        ) {
            let (mut read, mut write) = tokio::io::split(io);
            let payload = vec![send; sent];
            let mut result = vec![0; received];
            let (writes, reads) =
                tokio::join!(write.write_all(&payload), read.read_exact(&mut result));
            writes.unwrap();
            reads.unwrap();
            assert!(result.iter().all(|byte| *byte == receive));
        }
        let mut a = tokio::spawn(exchange(app_left, 1, 2, BLOCK * 80, BLOCK * 60));
        let mut b = tokio::spawn(exchange(app_right, 2, 1, BLOCK * 60, BLOCK * 80));
        tokio::time::timeout(TIMEOUT, async {
            let (mut a_done, mut b_done) = (false, false);
            while !a_done || !b_done {
                tokio::select! {
                    frame = left.next() => right.receive(frame.unwrap()).unwrap(),
                    frame = right.next() => left.receive(frame.unwrap()).unwrap(),
                    result = &mut a, if !a_done => { result.unwrap(); a_done = true; },
                    result = &mut b, if !b_done => { result.unwrap(); b_done = true; },
                }
            }
        })
        .await
        .expect("X11 credit window made no progress");
    }
}
