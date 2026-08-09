use std::{io, time::Duration};

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};

use crate::config::Config;

pub struct DaemonBridge {
    reader: Option<Lines<BufReader<OwnedReadHalf>>>,
    writer: Option<OwnedWriteHalf>,
}

/// A daemon event socket. Once subscribed, the daemon accepts no further RPC
/// requests on this connection and only writes event envelopes.
pub struct EventSubscription {
    reader: Lines<BufReader<OwnedReadHalf>>,
    // Keep the write side open: the daemon uses client disconnect to terminate
    // its event-stream loop.
    _writer: OwnedWriteHalf,
}

impl DaemonBridge {
    pub fn new() -> Self {
        Self {
            reader: None,
            writer: None,
        }
    }

    pub fn launch_if_needed() -> io::Result<()> {
        let Config {
            daemon_command,
            daemon_addr,
            ..
        } = Config::get();

        match std::net::TcpStream::connect(daemon_addr) {
            Ok(_) => Ok(()),
            Err(_) => {
                // std::process::Command::new(daemon_command).spawn()?;
                // Ok(())
								Err(io::Error::other("YOU NEED TO LAUNCH THE DAEMON MANUALLY FOR NOW. THIS IS A TEMPORARY HACK."))
            }
        }
    }

    async fn connect() -> Result<TcpStream, String> {
        const MAX_ATTEMPTS: usize = 8;
        const INITIAL_DELAY: Duration = Duration::from_millis(25);
        const MAX_DELAY: Duration = Duration::from_millis(250);

        let address = Config::get().daemon_addr.clone();
        let mut delay = INITIAL_DELAY;
        let mut last_error = None;

        for attempt in 0..MAX_ATTEMPTS {
            match TcpStream::connect(&address).await {
                Ok(stream) => return Ok(stream),
                Err(error) => last_error = Some(error),
            }
            if attempt + 1 < MAX_ATTEMPTS {
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2).min(MAX_DELAY);
            }
        }

        Err(format!(
            "daemon is unavailable at {address} after {MAX_ATTEMPTS} attempts: {}",
            last_error.expect("at least one connection attempt was made"),
        ))
    }

    async fn ensure_connected(&mut self) -> Result<(), String> {
        if self.reader.is_none() {
            let stream = Self::connect().await?;
            let (read_half, write_half) = stream.into_split();
            self.reader = Some(BufReader::new(read_half).lines());
            self.writer = Some(write_half);
        }
        Ok(())
    }

    /// Opens a distinct event-stream connection and verifies its acknowledgement.
    pub async fn subscribe(params: Value) -> Result<EventSubscription, String> {
        let stream = Self::connect().await?;
        let (read_half, mut writer) = stream.into_split();
        let mut reader = BufReader::new(read_half).lines();
        let request = format!(
            "{}\n",
            json!({ "method": crate::protocol::rpc::methods::SUBSCRIBE_EVENTS, "params": params })
        );
        writer
            .write_all(request.as_bytes())
            .await
            .map_err(|error| error.to_string())?;

        let acknowledgement = reader
            .next_line()
            .await
            .map_err(|error| error.to_string())?
            .ok_or("daemon closed the subscription connection")?;
        let response: Value =
            serde_json::from_str(&acknowledgement).map_err(|error| error.to_string())?;
        if let Some(error) = response.get("error").and_then(Value::as_str) {
            return Err(error.into());
        }
        if response
            .pointer("/result/subscribed")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err("daemon returned an invalid subscription acknowledgement".into());
        }

        Ok(EventSubscription {
            reader,
            _writer: writer,
        })
    }

    pub async fn call<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<T, String> {
        self.ensure_connected().await?;
        let request = format!("{}\n", json!({ "method": method, "params": params }));

        self.writer
            .as_mut()
            .expect("connection was initialized")
            .write_all(request.as_bytes())
            .await
            .map_err(|error| error.to_string())?;

        let line = self
            .reader
            .as_mut()
            .expect("connection was initialized")
            .next_line()
            .await
            .map_err(|error| error.to_string())?
            .ok_or("daemon closed the connection")?;
        let response: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;

        if let Some(error) = response.get("error").and_then(Value::as_str) {
            return Err(error.into());
        }
        serde_json::from_value(response.get("result").cloned().unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    }
}

impl EventSubscription {
    pub async fn next<T: DeserializeOwned>(&mut self) -> Result<T, String> {
        let line = self
            .reader
            .next_line()
            .await
            .map_err(|error| error.to_string())?
            .ok_or("daemon closed the subscription connection")?;
        let envelope: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
        serde_json::from_value(
            envelope
                .get("event")
                .cloned()
                .ok_or("invalid daemon event envelope")?,
        )
        .map_err(|error| error.to_string())
    }
}
