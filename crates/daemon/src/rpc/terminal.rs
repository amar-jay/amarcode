//! One authenticated WebSocket owns one interactive PTY; disconnect closes it.
use crate::{Error, Result};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    Open { cols: u16, rows: u16 },
    Resize { cols: u16, rows: u16 },
}
fn size(cols: u16, rows: u16) -> Result<PtySize> {
    if cols == 0 || rows == 0 || cols > 1000 || rows > 1000 {
        return Err(Error::msg("Terminal dimensions must be between 1 and 1000"));
    }
    Ok(PtySize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    })
}
struct ChildGuard {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    stop: Arc<AtomicBool>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        #[cfg(unix)]
        {
            use nix::{
                sys::signal::{killpg, Signal},
                unistd::Pid,
            };
            // An interactive command may own a different foreground process group.
            if let Some(group) = self.master.process_group_leader() {
                let _ = killpg(Pid::from_raw(group), Signal::SIGKILL);
            }
            if let Some(pid) = self.child.process_id() {
                if self.child.try_wait().ok().flatten().is_none() {
                    let _ = killpg(Pid::from_raw(pid as i32), Signal::SIGKILL);
                }
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) async fn serve<S>(socket: &mut WebSocketStream<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // No process until an authenticated client explicitly requests a terminal.
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
        .await
        .map_err(|_| Error::msg("Terminal open timed out"))?
        .ok_or_else(|| Error::msg("Terminal client disconnected"))?
        .map_err(|e| Error::msg(e.to_string()))?;
    let Control::Open { cols, rows } =
        serde_json::from_str(first.to_text().map_err(|e| Error::msg(e.to_string()))?)?
    else {
        return Err(Error::msg("Expected terminal open"));
    };
    let pair = native_pty_system()
        .openpty(size(cols, rows)?)
        .map_err(|e| Error::msg(e.to_string()))?;
    #[cfg(unix)]
    let shell = std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    #[cfg(windows)]
    let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
    let mut command = CommandBuilder::new(shell);
    #[cfg(unix)]
    command.arg("-l");
    command.env("TERM", "xterm-256color");
    // This PTY is independent of the terminal that launched the daemon.
    // Retaining these markers makes tmux/screen reject legitimate sessions.
    for variable in ["TMUX", "TMUX_PANE", "STY"] {
        command.env_remove(variable);
    }
    if let Some(home) = dirs::home_dir() {
        command.cwd(home);
    }
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| Error::msg(e.to_string()))?;
    drop(pair.slave);
    let mut session = ChildGuard {
        child,
        stop: Arc::new(AtomicBool::new(false)),
        master: pair.master,
    };
    let (mut reader, mut writer) = streams(session.master.as_ref())?;
    let (input, mut writes) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
    let stop = session.stop.clone();
    std::thread::spawn(move || {
        while let Some(bytes) = writes.blocking_recv() {
            let mut remaining = bytes.as_slice();
            while !remaining.is_empty() && !stop.load(Ordering::Relaxed) {
                match writer.write(remaining) {
                    Ok(0) => return,
                    Ok(count) => remaining = &remaining[count..],
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
    });
    let (tx, mut output) = tokio::sync::mpsc::channel(16);
    let stop = session.stop.clone();
    std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while !stop.load(Ordering::Relaxed) {
            match reader.read(&mut bytes) {
                Ok(0) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
                Ok(count) => {
                    if tx.blocking_send(bytes[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
    let mut last_seen = tokio::time::Instant::now();
    let mut exited = None;
    let mut last_output = tokio::time::Instant::now();
    let mut child_poll = tokio::time::interval(std::time::Duration::from_millis(100));
    loop {
        tokio::select! {
            _ = child_poll.tick() => {
                if let Some(code) = exited {
                    // Continue draining large output. A detached descendant may keep
                    // the PTY open after the shell exits, so bound idle drain time.
                    if output.is_empty() && last_output.elapsed() > std::time::Duration::from_secs(1) {
                        send_exit(socket, code).await?;
                        return Ok(());
                    }
                } else if let Some(status) = session.child.try_wait()? {
                    exited = Some(status.exit_code());
                    last_output = tokio::time::Instant::now();
                }
            }
            _ = heartbeat.tick() => {
                if last_seen.elapsed() > std::time::Duration::from_secs(45) { return Err(Error::msg("Terminal client timed out")); }
                tokio::time::timeout(std::time::Duration::from_secs(15), socket.send(Message::Ping(Vec::new().into())))
                    .await.map_err(|_| Error::msg("Terminal write timed out"))?
                    .map_err(|e| Error::msg(e.to_string()))?;
            }
            bytes = output.recv() => {
                match bytes {
                    Some(bytes) => {
                        last_output = tokio::time::Instant::now();
                        tokio::time::timeout(std::time::Duration::from_secs(15), socket.send(Message::Binary(bytes.into())))
                            .await.map_err(|_| Error::msg("Terminal write timed out"))?
                            .map_err(|e| Error::msg(e.to_string()))?;
                    }
                    None => {
                        let mut status = session.child.try_wait()?;
                        for _ in 0..50 {
                            if status.is_some() { break; }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            status = session.child.try_wait()?;
                        }
                        send_exit(socket, status.map(|s| s.exit_code()).unwrap_or(1)).await?;
                        return Ok(());
                    }
                }
            }
            // Pause incoming frames when the PTY input queue fills. This applies
            // backpressure during large pastes while output and cleanup keep running.
            frame = socket.next(), if input.capacity() > 0 => {
                last_seen = tokio::time::Instant::now();
                match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    if bytes.len() > 65536 { return Err(Error::msg("Terminal input too large")); }
                    input.try_send(bytes.to_vec()).map_err(|_| Error::msg("Terminal input is not being consumed"))?;
                }
                Some(Ok(Message::Text(text))) => match serde_json::from_str(&text)? {
                    Control::Resize { cols, rows } => session.master.resize(size(cols, rows)?).map_err(|e| Error::msg(e.to_string()))?,
                    Control::Open { .. } => return Err(Error::msg("Terminal is already open")),
                },
                Some(Ok(Message::Ping(bytes))) => {
                    tokio::time::timeout(std::time::Duration::from_secs(15), socket.send(Message::Pong(bytes)))
                        .await.map_err(|_| Error::msg("Terminal write timed out"))?
                        .map_err(|e| Error::msg(e.to_string()))?;
                },
                Some(Ok(Message::Pong(_))) => (),
                None | Some(Ok(Message::Close(_))) => return Ok(()),
                Some(Err(e)) => return Err(Error::msg(e.to_string())),
                _ => return Err(Error::msg("Invalid terminal frame")),
                }
            }
        }
    }
}

async fn send_exit<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    code: u32,
) -> Result<()> {
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        socket.send(Message::Text(
            serde_json::json!({"type":"exit","code":code})
                .to_string()
                .into(),
        )),
    )
    .await
    .map_err(|_| Error::msg("Terminal write timed out"))?
    .map_err(|e| Error::msg(e.to_string()))
}

// Unix PTY clones share file flags. Nonblocking I/O lets both workers stop
// even when a detached descendant keeps the slave open or stops reading input.
type Streams = (Box<dyn Read + Send>, Box<dyn Write + Send>);
#[cfg(unix)]
fn streams(master: &dyn portable_pty::MasterPty) -> Result<Streams> {
    use nix::fcntl::{fcntl, FcntlArg, OFlag};
    use std::os::fd::BorrowedFd;
    let raw = master
        .as_raw_fd()
        .ok_or_else(|| Error::msg("PTY has no descriptor"))?;
    // SAFETY: master owns this descriptor throughout the immediate clone call.
    // try_clone_to_owned also sets close-on-exec atomically.
    let owned = unsafe { BorrowedFd::borrow_raw(raw) }.try_clone_to_owned()?;
    let flags = fcntl(&owned, FcntlArg::F_GETFL).map_err(|e| Error::msg(e.to_string()))?;
    fcntl(
        &owned,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|e| Error::msg(e.to_string()))?;
    let reader = std::fs::File::from(owned);
    let writer = reader.try_clone()?;
    Ok((Box::new(reader), Box::new(writer)))
}
#[cfg(windows)]
fn streams(master: &dyn portable_pty::MasterPty) -> Result<Streams> {
    Ok((
        master
            .try_clone_reader()
            .map_err(|e| Error::msg(e.to_string()))?,
        master
            .take_writer()
            .map_err(|e| Error::msg(e.to_string()))?,
    ))
}
