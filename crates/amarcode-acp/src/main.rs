use std::{env, path::PathBuf};

mod provider;
mod runtime;
mod tools;

use provider::Config;

const DEFAULT_CONFIG_PATH: &str = "amarcode-acp.json";

#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    let config_path = config_path();
    let config = match Config::from_file(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("amarcode-acp: configuration error: {error}");
            return Ok(());
        }
    };

    runtime::serve(config).await
}

fn config_path() -> PathBuf {
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--config" {
            if let Some(path) = arguments.next() {
                return PathBuf::from(path);
            }
            eprintln!("amarcode-acp: --config requires a file path");
            break;
        }
    }
    PathBuf::from(DEFAULT_CONFIG_PATH)
}
