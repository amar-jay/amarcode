use std::{env, path::PathBuf};

mod provider;
mod runtime;
mod tools;

use provider::Config;

#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    let config_path = config_path();
		if config_path.is_none() {
			return Err(agent_client_protocol::Error::new(
				0x14, // 0x14 = 20 = INVALID_ARGUMENT
				"amarcode-acp: need to define --config flag with a file path".to_string()));
		}
		let config_path = config_path.unwrap();
    let config = match Config::from_file(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("amarcode-acp: configuration error: {error}");
            return Ok(());
        }
    };

    runtime::serve(config).await
}

fn config_path() -> Option<PathBuf> {
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--config" {
            if let Some(path) = arguments.next() {
                return Some(PathBuf::from(path));
            }
            eprintln!("amarcode-acp: --config requires a file path");
            break;
        }
    }
    eprintln!("amarcode-acp: need to define --config flag with a file path");
		return None;
}
