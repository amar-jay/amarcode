//! Machine policy is read from a fixed administrator-controlled location.
//! Environment variables and remote.json cannot override these restrictions.
use crate::{config::DaemonTransport, Config, Error, Result};
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Policy {
    pub allow_managed_tunnels: bool,
    pub allow_plaintext_loopback: bool,
    pub require_client_certificate: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            allow_managed_tunnels: true,
            allow_plaintext_loopback: true,
            require_client_certificate: false,
        }
    }
}
impl Policy {
    pub fn load() -> Result<Self> {
        #[cfg(target_os = "linux")]
        let path = Path::new("/etc/amarcode/security.json");
        #[cfg(target_os = "macos")]
        let path = Path::new("/Library/Application Support/Amarcode/security.json");
        #[cfg(windows)]
        let path = Path::new(r"C:\ProgramData\Amarcode\security.json");
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        return Ok(Self::default());
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            // Even a missing policy must not be removable/replaced through a
            // user-writable machine configuration directory.
            #[cfg(unix)]
            for directory in path.parent().into_iter().flat_map(Path::ancestors) {
                use std::os::unix::fs::MetadataExt;
                match fs::metadata(directory) {
                    Ok(metadata) if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 => {
                        return Err(Error::msg("Machine security policy directories must be root-owned and not group/other writable"));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                    _ => {}
                }
            }
            Self::read(path)
        }
    }
    fn read(path: &Path) -> Result<Self> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        if !metadata.is_file() {
            return Err(Error::msg(
                "Security policy must be a regular file, not a symlink",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                return Err(Error::msg(
                    "Security policy must be root-owned and not writable by group or others",
                ));
            }
        }
        serde_json::from_slice(&fs::read(path)?)
            .map_err(|e| Error::msg(format!("Invalid machine security policy: {e}")))
    }
    pub fn validate(&self, config: &Config) -> Result<()> {
        if !self.allow_plaintext_loopback
            && (config.transport == DaemonTransport::Tcp || config.ws_tls_cert.is_none())
        {
            return Err(Error::msg(
                "Machine security policy prohibits plaintext listeners; configure WebSocket TLS",
            ));
        }
        if self.require_client_certificate
            && (config.transport != DaemonTransport::WebSocket || config.ws_client_ca.is_none())
        {
            return Err(Error::msg(
                "Machine security policy requires mutual TLS; configure AMARCODE_WS_CLIENT_CA",
            ));
        }
        Ok(())
    }
    pub fn require_tunnel_allowed(&self) -> Result<()> {
        if !self.allow_managed_tunnels {
            return Err(Error::msg(
                "Machine security policy prohibits managed Cloudflare tunnels",
            ));
        }
        if self.require_client_certificate {
            return Err(Error::msg("Managed tunnels cannot satisfy mandatory client-certificate authentication; use direct mTLS"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            app_dir: Default::default(),
            db_path: Default::default(),
            daemon_addr: "127.0.0.1:0".into(),
            transport: DaemonTransport::Tcp,
            default_workspace_path: None,
            acp_registry_source: None,
            ws_token_file: None,
            ws_tls_cert: None,
            ws_tls_key: None,
            ws_client_ca: None,
            ws_allowed_origins: vec![],
        }
    }
    #[test]
    fn restrictions_fail_closed_for_plaintext_and_tunnels() {
        let policy: Policy = serde_json::from_str(r#"{"allow_managed_tunnels":false,"allow_plaintext_loopback":false,"require_client_certificate":true}"#).unwrap();
        let mut config = config();
        assert!(policy.validate(&config).is_err());
        config.transport = DaemonTransport::WebSocket;
        config.ws_tls_cert = Some("server.pem".into());
        assert!(policy.validate(&config).is_err());
        config.ws_client_ca = Some("ca.pem".into());
        assert!(policy.validate(&config).is_ok());
        assert!(policy.require_tunnel_allowed().is_err());
        assert!(Policy {
            require_client_certificate: true,
            ..Default::default()
        }
        .require_tunnel_allowed()
        .is_err());
        assert!(serde_json::from_str::<Policy>(r#"{"allow_managed_tunels":true}"#).is_err());
    }
    #[test]
    fn missing_policy_defaults_and_nonregular_policies_fail() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            Policy::read(&root.path().join("missing"))
                .unwrap()
                .allow_managed_tunnels
        );
        assert!(Policy::read(root.path()).is_err());
    }
}
