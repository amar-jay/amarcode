use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::config::{
    CURRENT_MANIFEST_FILE, CURRENT_SIGNATURE_FILE, DEFAULT_RELEASE_MANIFEST_URL,
    RELEASE_PUBLIC_KEY_HEX,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseManifest {
    pub version: String,
    pub protocol_version: u32,
    pub artifacts: HashMap<String, ReleaseArtifact>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseArtifact {
    pub target: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct VerifiedManifest {
    pub manifest: ReleaseManifest,
    bytes: Vec<u8>,
    signature: String,
    source_url: reqwest::Url,
}

pub fn platform_target() -> Result<&'static str, String> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok("x86_64-unknown-linux-gnu");

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok("x86_64-pc-windows-gnu");

    #[allow(unreachable_code)]
    Err(format!(
        "daemon downloads are not published for {}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS
    ))
}

pub async fn fetch_latest(client: &reqwest::Client) -> Result<VerifiedManifest, String> {
    let url = std::env::var("AMARCODE_DAEMON_RELEASE_URL")
        .unwrap_or_else(|_| DEFAULT_RELEASE_MANIFEST_URL.to_string());
    let manifest_url = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
    let signature_url =
        reqwest::Url::parse(&format!("{url}.sig")).map_err(|error| error.to_string())?;

    let manifest_response = client
        .get(manifest_url.clone())
        .send()
        .await
        .map_err(|error| format!("failed to download daemon manifest: {error}"))?
        .error_for_status()
        .map_err(|error| format!("failed to download daemon manifest: {error}"))?;
    if manifest_response.content_length().unwrap_or(0) > 1024 * 1024 {
        return Err("daemon manifest exceeds the 1 MiB safety limit".into());
    }
    let signature_response = client
        .get(signature_url)
        .send()
        .await
        .map_err(|error| format!("failed to download daemon manifest signature: {error}"))?
        .error_for_status()
        .map_err(|error| format!("failed to download daemon manifest signature: {error}"))?;
    if signature_response.content_length().unwrap_or(0) > 4096 {
        return Err("daemon manifest signature exceeds the safety limit".into());
    }

    let bytes = manifest_response
        .bytes()
        .await
        .map_err(|error| format!("failed to read daemon manifest: {error}"))?
        .to_vec();
    let signature = signature_response
        .text()
        .await
        .map_err(|error| format!("failed to read daemon manifest signature: {error}"))?;
    verified_manifest(bytes, signature, manifest_url)
}

pub fn load_cached(install_root: &Path) -> Result<VerifiedManifest, String> {
    let bytes = fs::read(install_root.join(CURRENT_MANIFEST_FILE))
        .map_err(|error| format!("failed to read cached daemon manifest: {error}"))?;
    let signature = fs::read_to_string(install_root.join(CURRENT_SIGNATURE_FILE))
        .map_err(|error| format!("failed to read cached daemon signature: {error}"))?;
    let source_url =
        reqwest::Url::parse(DEFAULT_RELEASE_MANIFEST_URL).map_err(|error| error.to_string())?;
    verified_manifest(bytes, signature, source_url)
}

fn verified_manifest(
    bytes: Vec<u8>,
    signature_text: String,
    source_url: reqwest::Url,
) -> Result<VerifiedManifest, String> {
    let public_key = hex::decode(RELEASE_PUBLIC_KEY_HEX)
        .map_err(|error| format!("invalid embedded daemon public key: {error}"))?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| "invalid embedded daemon public key length".to_string())?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|error| format!("invalid embedded daemon public key: {error}"))?;
    let signature_bytes = BASE64
        .decode(signature_text.trim())
        .map_err(|error| format!("invalid daemon manifest signature encoding: {error}"))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|error| format!("invalid daemon manifest signature: {error}"))?;
    verifying_key
        .verify(&bytes, &signature)
        .map_err(|_| "daemon manifest signature verification failed".to_string())?;

    let manifest: ReleaseManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid daemon manifest: {error}"))?;
    validate_segment("version", &manifest.version)?;
    for (target, artifact) in &manifest.artifacts {
        validate_segment("target", target)?;
        if artifact.target != *target {
            return Err(format!("daemon artifact target mismatch for {target}"));
        }
        if artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("invalid daemon checksum for {target}"));
        }
    }

    Ok(VerifiedManifest {
        manifest,
        bytes,
        signature: signature_text.trim().to_string(),
        source_url,
    })
}

impl VerifiedManifest {
    pub fn artifact(&self, target: &str) -> Result<&ReleaseArtifact, String> {
        self.manifest.artifacts.get(target).ok_or_else(|| {
            format!(
                "daemon release {} has no artifact for {target}",
                self.manifest.version
            )
        })
    }

    pub async fn ensure_installed(
        &self,
        client: &reqwest::Client,
        install_root: &Path,
        target: &str,
    ) -> Result<PathBuf, String> {
        let artifact = self.artifact(target)?;
        let filename = if target.contains("windows") {
            "amarcode-daemon.exe"
        } else {
            "amarcode-daemon"
        };
        let install_dir = install_root.join(&self.manifest.version).join(target);
        let destination = install_dir.join(filename);
        if destination.is_file() && verify_file(&destination, artifact)? {
            return Ok(destination);
        }

        let download_url = self
            .source_url
            .join(&artifact.url)
            .map_err(|error| format!("invalid daemon artifact URL: {error}"))?;
        let response = client
            .get(download_url)
            .send()
            .await
            .map_err(|error| format!("failed to download daemon: {error}"))?
            .error_for_status()
            .map_err(|error| format!("failed to download daemon: {error}"))?;
        if let Some(length) = response.content_length() {
            if length != artifact.size {
                return Err(format!(
                    "daemon response size mismatch: expected {}, received {length}",
                    artifact.size
                ));
            }
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("failed to read daemon download: {error}"))?;
        verify_bytes(&bytes, artifact)?;

        fs::create_dir_all(&install_dir)
            .map_err(|error| format!("failed to create daemon install directory: {error}"))?;
        let temporary = install_dir.join(format!("{filename}.{}.part", std::process::id()));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .map_err(|error| format!("failed to clear stale daemon download: {error}"))?;
        }
        write_executable(&temporary, &bytes)?;
        if destination.exists() {
            fs::remove_file(&destination)
                .map_err(|error| format!("failed to replace daemon executable: {error}"))?;
        }
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("failed to install daemon executable: {error}"))?;
        Ok(destination)
    }

    pub fn save_as_current(&self, install_root: &Path) -> Result<(), String> {
        fs::create_dir_all(install_root)
            .map_err(|error| format!("failed to create daemon install directory: {error}"))?;
        atomic_write(&install_root.join(CURRENT_MANIFEST_FILE), &self.bytes)?;
        atomic_write(
            &install_root.join(CURRENT_SIGNATURE_FILE),
            format!("{}\n", self.signature).as_bytes(),
        )
    }
}

fn verify_file(path: &Path, artifact: &ReleaseArtifact) -> Result<bool, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read cached daemon {}: {error}", path.display()))?;
    Ok(verify_bytes(&bytes, artifact).is_ok())
}

fn verify_bytes(bytes: &[u8], artifact: &ReleaseArtifact) -> Result<(), String> {
    if bytes.len() as u64 != artifact.size {
        return Err(format!(
            "daemon download size mismatch: expected {}, received {}",
            artifact.size,
            bytes.len()
        ));
    }
    let actual = hex::encode(Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(&artifact.sha256) {
        return Err("daemon download checksum verification failed".into());
    }
    Ok(())
}

fn write_executable(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("failed to create daemon download: {error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("failed to write daemon download: {error}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("failed to make daemon executable: {error}"))?;
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.part", std::process::id()));
    let mut file = File::create(&temporary)
        .map_err(|error| format!("failed to create {}: {error}", temporary.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("failed to replace {}: {error}", path.display()))?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| format!("failed to install {}: {error}", path.display()))
}

fn validate_segment(label: &str, value: &str) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(format!("invalid daemon {label}: {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNED_MANIFEST: &str =
        "{\"version\":\"0.1.0\",\"protocolVersion\":1,\"artifacts\":{}}\n";
    const SIGNATURE: &str =
        "GQgA9igK36F5ZYEeLMv3gFhW/AovXsVQRTiV71xvjTOa01T7E5rknhzNSu3eab3sn8b7CTXQXC85Iy+BxsTVBQ==";

    #[test]
    fn accepts_a_manifest_signed_by_the_release_key() {
        let url = reqwest::Url::parse(DEFAULT_RELEASE_MANIFEST_URL).unwrap();
        let verified =
            verified_manifest(SIGNED_MANIFEST.as_bytes().to_vec(), SIGNATURE.into(), url)
                .expect("fixture signature should verify");
        assert_eq!(verified.manifest.version, "0.1.0");
        assert_eq!(verified.manifest.protocol_version, 1);
    }

    #[test]
    fn rejects_a_tampered_signed_manifest() {
        let url = reqwest::Url::parse(DEFAULT_RELEASE_MANIFEST_URL).unwrap();
        let tampered = SIGNED_MANIFEST.replace("0.1.0", "0.1.1");
        let error = verified_manifest(tampered.into_bytes(), SIGNATURE.into(), url)
            .expect_err("tampering must invalidate the signature");
        assert!(error.contains("signature verification failed"));
    }

    #[test]
    fn rejects_unsafe_path_segments() {
        assert!(validate_segment("version", "0.1.0").is_ok());
        assert!(validate_segment("version", "../latest").is_err());
        assert!(validate_segment("target", "windows/x64").is_err());
    }

    #[test]
    #[ignore = "requires the live Cloudflare release endpoint"]
    fn downloads_and_verifies_the_live_linux_release() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(90))
                .build()
                .unwrap();
            let manifest = fetch_latest(&client).await.unwrap();
            let target = "x86_64-unknown-linux-gnu";
            let install_root = std::env::temp_dir().join(format!(
                "amarcode-release-integration-{}",
                std::process::id()
            ));
            let executable = manifest
                .ensure_installed(&client, &install_root, target)
                .await
                .unwrap();
            assert!(verify_file(&executable, manifest.artifact(target).unwrap()).unwrap());
            std::fs::remove_dir_all(install_root).unwrap();
        });
    }
}
