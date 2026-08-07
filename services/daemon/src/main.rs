use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{UnixListener, UnixStream}};

const SERVICE_NAME: &str = "acp-workbench-daemon";

#[derive(Debug, Deserialize)]
struct Request {
    method: String,
}

#[derive(Debug, Serialize)]
struct Response<'a> {
    service: &'a str,
    status: &'a str,
    version: &'a str,
}

fn socket_path() -> PathBuf {
    std::env::var_os("ACP_WORKBENCH_DAEMON_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("acp-workbench-daemon.sock"))
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let socket = socket_path();
    if socket.exists() {
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    eprintln!("{SERVICE_NAME} listening on {}", socket.display());

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                tokio::spawn(handle_connection(stream));
            }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                std::fs::remove_file(&socket)?;
                return Ok(());
            }
        }
    }
}

async fn handle_connection(stream: UnixStream) -> io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request { method }) if method == "health" => serde_json::to_string(&Response {
                service: SERVICE_NAME,
                status: "ready",
                version: env!("CARGO_PKG_VERSION"),
            }),
            Ok(_) => Ok(r#"{"error":"unknown method"}"#.to_owned()),
            Err(_) => Ok(r#"{"error":"invalid request"}"#.to_owned()),
        }.unwrap();
        writer.write_all(response.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_socket_can_be_overridden() {
        std::env::set_var("ACP_WORKBENCH_DAEMON_SOCKET", "/tmp/acp-workbench-test.sock");
        assert_eq!(socket_path(), PathBuf::from("/tmp/acp-workbench-test.sock"));
        std::env::remove_var("ACP_WORKBENCH_DAEMON_SOCKET");
    }
}
