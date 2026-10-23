//! Signed daemon release storage shared by bootstrap and the independent updater.
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CURRENT_MANIFEST_FILE: &str = "current-manifest.json";
pub const CURRENT_SIGNATURE_FILE: &str = "current-manifest.json.sig";
pub const DEFAULT_RELEASE_MANIFEST_URL: &str =
    "https://updates.amarcode.amarjay.com/v1/daemon/latest.json";
pub const RELEASE_PUBLIC_KEY_HEX: &str =
    "5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2";

#[derive(Serialize, Deserialize)]
struct StoredRelease {
    manifest: String,
    signature: String,
    source_url: String,
}

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

    let bytes = bounded_body(manifest_response, 1024 * 1024).await?;
    let signature = String::from_utf8(bounded_body(signature_response, 4096).await?)
        .map_err(|error| error.to_string())?;
    verified_manifest(bytes, signature, manifest_url)
}

pub fn load_cached(install_root: &Path) -> Result<VerifiedManifest, String> {
    // A single atomic record commits the manifest, signature and source URL
    // together. Fall back to the original pair only for pre-migration installs.
    let record = install_root.join("current-release.json");
    match fs::read(&record) {
        Ok(bytes) => {
            let saved: StoredRelease = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            return verified_manifest(
                saved.manifest.into_bytes(),
                saved.signature,
                reqwest::Url::parse(&saved.source_url).map_err(|e| e.to_string())?,
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
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
        if artifact.size == 0 || artifact.size > 512 * 1024 * 1024 {
            return Err("daemon artifact size is outside the supported range".into());
        }
        let bytes = bounded_body(response, artifact.size).await?;
        verify_bytes(&bytes, artifact)?;

        fs::create_dir_all(&install_dir)
            .map_err(|error| format!("failed to create daemon install directory: {error}"))?;
        let temporary = install_dir.join(format!("{filename}.{}.part", std::process::id()));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .map_err(|error| format!("failed to clear stale daemon download: {error}"))?;
        }
        write_executable(&temporary, &bytes)?;
        // Never replace an executing binary in place. Versioned releases are
        // immutable; a corrupt file must be repaired before it is used.
        if destination.exists() {
            fs::remove_file(&destination).map_err(|e| e.to_string())?;
        }
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("failed to install daemon executable: {error}"))?;
        Ok(destination)
    }

    /// Return the verified on-disk executable without downloading or writing.
    /// This is used by ordinary bootstrap, which must remain read-only.
    pub fn installed_executable(
        &self,
        install_root: &Path,
        target: &str,
    ) -> Result<Option<PathBuf>, String> {
        let artifact = self.artifact(target)?;
        let filename = if target.contains("windows") {
            "amarcode-daemon.exe"
        } else {
            "amarcode-daemon"
        };
        let path = install_root
            .join(&self.manifest.version)
            .join(target)
            .join(filename);
        if !path.is_file() {
            return Ok(None);
        }
        verify_file(&path, artifact).map(|verified| verified.then_some(path))
    }

    pub fn save_as_current(&self, install_root: &Path) -> Result<(), String> {
        fs::create_dir_all(install_root)
            .map_err(|error| format!("failed to create daemon install directory: {error}"))?;
        let saved = StoredRelease {
            manifest: String::from_utf8(self.bytes.clone()).map_err(|e| e.to_string())?,
            signature: self.signature.clone(),
            source_url: self.source_url.to_string(),
        };
        // Keep pre-job desktop clients' cache readable. New clients and the
        // supervisor use the coherent record below as the sole commit point.
        atomic_write(&install_root.join(CURRENT_MANIFEST_FILE), &self.bytes)?;
        atomic_write(
            &install_root.join(CURRENT_SIGNATURE_FILE),
            self.signature.as_bytes(),
        )?;
        atomic_write(
            &install_root.join("current-release.json"),
            &serde_json::to_vec(&saved).map_err(|e| e.to_string())?,
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

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.part", std::process::id()));
    let mut file = File::create(&temporary)
        .map_err(|error| format!("failed to create {}: {error}", temporary.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("failed to install {}: {error}", path.display()))?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn bounded_body(mut response: reqwest::Response, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > limit {
            return Err("release response exceeds its size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_segment(label: &str, value: &str) -> Result<(), String> {
    let valid = value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
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

/// Read-only progress fallback while the daemon's RPC listener is restarting.
/// Only the supervisor writes these snapshots; clients never drive recovery.
pub fn read_update_job(
    root: &Path,
    id: Option<&str>,
) -> Result<Option<amarcode_protocol::rpc::DaemonUpdateJob>, String> {
    let id = match id {
        Some(id) => id.to_owned(),
        None => match fs::read_to_string(root.join("updates/latest")) {
            Ok(id) => id,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        },
    };
    if id.len() != 36 || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("invalid daemon update job id".into());
    }
    let bytes =
        fs::read(root.join("updates").join(&id).join("status.json")).map_err(|e| e.to_string())?;
    let job: amarcode_protocol::rpc::DaemonUpdateJob =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if job.id != id {
        return Err("daemon update job id mismatch".into());
    }
    Ok(Some(job))
}

/// Serialize bootstrap repairs against daemon job creation. The independent
/// supervisor has its own lock, so a restart never transfers a workspace lock.
pub fn lock_update(root: &Path, name: &str) -> Result<File, String> {
    if !matches!(name, "request.lock" | "supervisor.lock") {
        return Err("invalid update lock name".into());
    }
    fs::create_dir_all(root.join("updates")).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("updates").join(name))
        .map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "another daemon update operation is active".to_string())?;
    Ok(file)
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

#[cfg(test)]
mod storage_tests {
    use super::*;
    const MANIFEST: &str = "{\"version\":\"0.1.0\",\"protocolVersion\":1,\"artifacts\":{}}\n";
    const SIGNATURE: &str =
        "GQgA9igK36F5ZYEeLMv3gFhW/AovXsVQRTiV71xvjTOa01T7E5rknhzNSu3eab3sn8b7CTXQXC85Iy+BxsTVBQ==";
    #[test]
    fn migrates_legacy_cache_to_one_atomic_signed_record_and_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CURRENT_MANIFEST_FILE), MANIFEST).unwrap();
        fs::write(dir.path().join(CURRENT_SIGNATURE_FILE), SIGNATURE).unwrap();
        let old = load_cached(dir.path()).unwrap();
        old.save_as_current(dir.path()).unwrap();
        fs::remove_file(dir.path().join(CURRENT_MANIFEST_FILE)).unwrap();
        fs::remove_file(dir.path().join(CURRENT_SIGNATURE_FILE)).unwrap();
        let loaded = load_cached(dir.path()).unwrap();
        assert_eq!(loaded.bytes, MANIFEST.as_bytes());
        let mut custom = loaded;
        custom.source_url = reqwest::Url::parse("https://example.com/custom/latest.json").unwrap();
        custom.save_as_current(dir.path()).unwrap();
        assert_eq!(
            load_cached(dir.path()).unwrap().source_url,
            custom.source_url
        );
    }
    #[test]
    fn uncommitted_temporary_file_cannot_replace_current_release() {
        let dir = tempfile::tempdir().unwrap();
        let verified = verified_manifest(
            MANIFEST.as_bytes().to_vec(),
            SIGNATURE.into(),
            reqwest::Url::parse(DEFAULT_RELEASE_MANIFEST_URL).unwrap(),
        )
        .unwrap();
        verified.save_as_current(dir.path()).unwrap();
        fs::write(
            dir.path().join("current-release.interrupted.part"),
            "corrupted",
        )
        .unwrap();
        assert_eq!(load_cached(dir.path()).unwrap().manifest.version, "0.1.0");
        // A corrupt committed record must not silently fall back to an older
        // pair, which could otherwise conceal a torn or tampered activation.
        fs::write(dir.path().join(CURRENT_MANIFEST_FILE), MANIFEST).unwrap();
        fs::write(dir.path().join(CURRENT_SIGNATURE_FILE), SIGNATURE).unwrap();
        fs::write(dir.path().join("current-release.json"), "corrupted").unwrap();
        assert!(load_cached(dir.path()).is_err());
    }
    #[test]
    fn release_identifiers_cannot_name_parent_or_current_directory() {
        for value in [".", "..", "../version", "/version", ""] {
            assert!(validate_segment("version", value).is_err());
        }
    }
}
