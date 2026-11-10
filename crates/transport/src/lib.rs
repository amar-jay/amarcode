//! Shared native-client TLS configuration. Certificate verification is always on.
pub mod encrypted;
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};
pub use tokio_rustls::rustls;
const INNER_VERSIONS: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

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
    client_config_with_crl(
        path("AMARCODE_CLIENT_TLS_CA").as_deref(),
        path("AMARCODE_CLIENT_TLS_CERT").as_deref(),
        path("AMARCODE_CLIENT_TLS_KEY").as_deref(),
        path("AMARCODE_CLIENT_TLS_CRL").as_deref(),
        false,
    )
}
pub fn client_config_with(
    ca: Option<&Path>,
    cert: Option<&Path>,
    key: Option<&Path>,
) -> Result<rustls::ClientConfig, String> {
    client_config_with_crl(ca, cert, key, None, false)
}

pub fn client_config_with_crl(
    ca: Option<&Path>,
    cert: Option<&Path>,
    key: Option<&Path>,
    crl: Option<&Path>,
    inner: bool,
) -> Result<rustls::ClientConfig, String> {
    if cert.is_some() != key.is_some() {
        return Err("Client TLS requires both certificate and private key".into());
    }
    let mut roots = rustls::RootCertStore {
        roots: if inner {
            vec![]
        } else {
            webpki_roots::TLS_SERVER_ROOTS.to_vec()
        },
    };
    if let Some(ca) = ca {
        for certificate in certificates(ca)? {
            roots.add(certificate).map_err(|e| e.to_string())?;
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(if inner {
            INNER_VERSIONS
        } else {
            rustls::DEFAULT_VERSIONS
        })
        .map_err(|e| e.to_string())?;
    let builder = if let Some(crl) = crl {
        let verifier =
            rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
                .with_crls(crls(crl)?)
                .enforce_revocation_expiration()
                .build()
                .map_err(|e| e.to_string())?;
        builder.with_webpki_verifier(verifier)
    } else {
        builder.with_root_certificates(roots)
    };
    let mut config = match (cert, key) {
        (Some(cert), Some(key)) => builder
            .with_client_auth_cert(certificates(cert)?, private_key(key)?)
            .map_err(|e| e.to_string())?,
        _ => builder.with_no_client_auth(),
    };
    // Rechecking CRLs on every connection must not be bypassed by TLS resumption.
    config.resumption = rustls::client::Resumption::disabled();
    Ok(config)
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

pub fn crls(
    path: &Path,
) -> Result<Vec<rustls::pki_types::CertificateRevocationListDer<'static>>, String> {
    let values = rustls_pemfile::crls(&mut BufReader::new(
        File::open(path).map_err(|e| e.to_string())?,
    ))
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    if values.is_empty() {
        return Err("Configured CRL file contains no PEM revocation lists".into());
    }
    Ok(values)
}

#[derive(Clone, Debug)]
pub struct TlsIdentity {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub client_ca: Option<PathBuf>,
    pub client_crl: Option<PathBuf>,
}
impl TlsIdentity {
    pub fn client_verifier(
        &self,
    ) -> Result<Arc<dyn rustls::server::danger::ClientCertVerifier>, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = if let Some(ca) = &self.client_ca {
            let mut roots = rustls::RootCertStore::empty();
            for cert in certificates(ca)? {
                roots.add(cert).map_err(|e| e.to_string())?;
            }
            let mut builder = rustls::server::WebPkiClientVerifier::builder_with_provider(
                Arc::new(roots),
                Arc::clone(&provider),
            );
            if let Some(crl) = &self.client_crl {
                builder = builder
                    .with_crls(crls(crl)?)
                    .enforce_revocation_expiration();
            }
            builder.build().map_err(|e| e.to_string())?
        } else {
            rustls::server::WebPkiClientVerifier::no_client_auth()
        };
        Ok(verifier)
    }
    pub fn verify_client(
        &self,
        chain: &[rustls::pki_types::CertificateDer<'static>],
    ) -> Result<(), String> {
        let (leaf, intermediates) = chain
            .split_first()
            .ok_or("Missing paired client identity")?;
        self.client_verifier()?
            .verify_client_cert(leaf, intermediates, rustls::pki_types::UnixTime::now())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn server_config(&self, inner: bool) -> Result<rustls::ServerConfig, String> {
        if self.client_crl.is_some() && self.client_ca.is_none() {
            return Err("Client CRL requires a client CA".into());
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = self.client_verifier()?;
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(if inner {
                INNER_VERSIONS
            } else {
                rustls::DEFAULT_VERSIONS
            })
            .map_err(|e| e.to_string())?
            .with_client_cert_verifier(verifier)
            .with_single_cert(certificates(&self.cert)?, private_key(&self.key)?)
            .map_err(|e| e.to_string())?;
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        config.send_tls13_tickets = 0;
        Ok(config)
    }
}
