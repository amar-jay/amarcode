//! Shared native-client TLS configuration. Certificate verification is always on.
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};
pub use tokio_rustls::rustls;

pub fn certificates(
    path: &Path,
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    let certs = rustls_pemfile::certs(&mut BufReader::new(
        File::open(path).map_err(|e| e.to_string())?,
    ))
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    if certs.is_empty() {
        return Err(format!("No PEM certificates in {}", path.display()));
    }
    Ok(certs)
}
pub fn private_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err("TLS private key must not be accessible to group or other users".into());
        }
    }
    rustls_pemfile::private_key(&mut BufReader::new(
        File::open(path).map_err(|e| e.to_string())?,
    ))
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "No PEM private key".into())
}
fn path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
pub fn client_config() -> Result<rustls::ClientConfig, String> {
    client_config_with(
        path("AMARCODE_CLIENT_TLS_CA").as_deref(),
        path("AMARCODE_CLIENT_TLS_CERT").as_deref(),
        path("AMARCODE_CLIENT_TLS_KEY").as_deref(),
    )
}
pub fn client_config_with(
    ca: Option<&Path>,
    cert: Option<&Path>,
    key: Option<&Path>,
) -> Result<rustls::ClientConfig, String> {
    if cert.is_some() != key.is_some() {
        return Err("Client TLS requires both certificate and private key".into());
    }
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(ca) = ca {
        for certificate in certificates(ca)? {
            roots.add(certificate).map_err(|e| e.to_string())?;
        }
    }
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots);
    match (cert, key) {
        (Some(cert), Some(key)) => builder
            .with_client_auth_cert(certificates(cert)?, private_key(key)?)
            .map_err(|e| e.to_string()),
        _ => Ok(builder.with_no_client_auth()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_or_missing_explicit_credentials_fail_closed() {
        assert!(client_config_with(None, Some(Path::new("missing-cert")), None).is_err());
        assert!(client_config_with(None, None, Some(Path::new("missing-key"))).is_err());
        assert!(client_config_with(Some(Path::new("missing-ca")), None, None).is_err());
    }
}
