//! A managed installation originates either in a signed desktop bundle or a
//! signed downloaded manifest. Only the local bootstrap can create bundle
//! records; downloaded releases always require the pinned release signature.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundledRelease {
    version: String,
    protocol_version: u32,
    target: String,
    sha256: String,
    size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct BundleRecord {
    bundled: BundledRelease,
}

#[derive(Clone, Debug)]
pub enum InstalledRelease {
    Signed(VerifiedManifest),
    Bundled(BundledRelease),
}

impl InstalledRelease {
    pub fn version(&self) -> &str {
        match self {
            Self::Signed(release) => &release.manifest.version,
            Self::Bundled(release) => &release.version,
        }
    }
    pub fn protocol_version(&self) -> u32 {
        match self {
            Self::Signed(release) => release.manifest.protocol_version,
            Self::Bundled(release) => release.protocol_version,
        }
    }
    pub fn installed_executable(&self, root: &Path) -> Result<PathBuf, String> {
        match self {
            Self::Signed(release) => release
                .installed_executable(root, platform_target()?)?
                .ok_or("verified installed daemon executable is missing".into()),
            Self::Bundled(release) => {
                release.validate()?;
                let path = release.path(root);
                let artifact = ReleaseArtifact {
                    target: release.target.clone(),
                    url: String::new(),
                    sha256: release.sha256.clone(),
                    size: release.size,
                };
                if !verify_file(&path, &artifact)? {
                    return Err("bundled daemon checksum verification failed".into());
                }
                Ok(path)
            }
        }
    }
    pub fn save_as_current(&self, root: &Path) -> Result<(), String> {
        match self {
            Self::Signed(release) => release.save_as_current(root),
            Self::Bundled(release) => {
                release.validate()?;
                fs::create_dir_all(root).map_err(|e| e.to_string())?;
                atomic_write(
                    &root.join("current-release.json"),
                    &serde_json::to_vec(&BundleRecord {
                        bundled: release.clone(),
                    })
                    .map_err(|e| e.to_string())?,
                )
            }
        }
    }
}
impl BundledRelease {
    fn validate(&self) -> Result<(), String> {
        validate_segment("version", &self.version)?;
        if self.target != platform_target()?
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.size == 0
        {
            return Err("invalid bundled daemon record".into());
        }
        if let Some(instance) = &self.instance {
            uuid::Uuid::parse_str(instance).map_err(|_| "invalid bundle instance")?;
        }
        Ok(())
    }
    fn path(&self, root: &Path) -> PathBuf {
        let mut directory = root.join("bundled").join(&self.version).join(&self.sha256);
        if let Some(instance) = &self.instance {
            directory = directory.join(instance);
        }
        directory.join(format!("amarcode-daemon{}", std::env::consts::EXE_SUFFIX))
    }
}

pub fn load_installed(root: &Path) -> Result<InstalledRelease, String> {
    match fs::read(root.join("current-release.json")) {
        Ok(bytes) => {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if value.get("bundled").is_some() {
                let record: BundleRecord =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                record.bundled.validate()?;
                return Ok(InstalledRelease::Bundled(record.bundled));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    load_cached(root).map(InstalledRelease::Signed)
}

/// Copy the executable supplied by the desktop package to immutable managed
/// storage. Its trust comes from the desktop package, not a fabricated release
/// signature. The hash verifies later starts and rollback, including same-version
/// desktop rebuilds. Never register a service against the app installation path.
pub fn stage_bundle(root: &Path) -> Result<InstalledRelease, String> {
    stage_bundle_from(root, &std::env::current_exe().map_err(|e| e.to_string())?)
}
pub(crate) fn stage_bundle_from(root: &Path, source: &Path) -> Result<InstalledRelease, String> {
    let bytes = fs::read(source).map_err(|e| e.to_string())?;
    let mut bundled = BundledRelease {
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: amarcode_protocol::PROTOCOL_VERSION,
        target: platform_target()?.into(),
        sha256: hex::encode(Sha256::digest(&bytes)),
        size: bytes.len() as u64,
        instance: None,
    };
    bundled.validate()?;
    if InstalledRelease::Bundled(bundled.clone())
        .installed_executable(root)
        .is_ok()
    {
        return Ok(InstalledRelease::Bundled(bundled));
    }
    // Never replace a mapped/running executable. A damaged deterministic copy
    // gets a fresh immutable instance, so explicit repair works on Windows too.
    if bundled.path(root).exists() {
        bundled.instance = Some(uuid::Uuid::new_v4().to_string());
    }
    let path = bundled.path(root);
    let release = InstalledRelease::Bundled(bundled);
    fs::create_dir_all(path.parent().ok_or("missing bundle parent")?).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.part", std::process::id()));
    if temporary.exists() {
        fs::remove_file(&temporary).map_err(|e| e.to_string())?;
    }
    write_executable(&temporary, &bytes)?;
    fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    release.installed_executable(root)?;
    Ok(release)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_install_is_offline_independent_and_retained_for_rollback() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("desktop-sidecar");
        fs::write(&source, b"bundled executable fixture").unwrap();
        let store = root.path().join("daemon");
        let release = stage_bundle_from(&store, &source).unwrap();
        release.save_as_current(&store).unwrap();
        fs::remove_file(source).unwrap();
        let installed = load_installed(&store).unwrap();
        assert_eq!(installed.version(), env!("CARGO_PKG_VERSION"));
        let executable = installed.installed_executable(&store).unwrap();
        let snapshot = root.path().join("rollback");
        installed.save_as_current(&snapshot).unwrap();
        assert_eq!(
            load_installed(&snapshot)
                .unwrap()
                .installed_executable(&store)
                .unwrap(),
            executable
        );
        // A bundle record is never accepted as a signed download manifest.
        assert!(load_cached(&store).is_err());
        fs::write(executable, b"tampered").unwrap();
        assert!(load_installed(&snapshot)
            .unwrap()
            .installed_executable(&store)
            .is_err());
    }
    #[test]
    fn repair_stages_a_fresh_instance_instead_of_overwriting_a_damaged_executable() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("desktop");
        fs::write(&source, b"trusted bundle").unwrap();
        let original = stage_bundle_from(root.path(), &source).unwrap();
        let damaged = original.installed_executable(root.path()).unwrap();
        fs::write(&damaged, b"corrupt").unwrap();
        let repaired = stage_bundle_from(root.path(), &source).unwrap();
        let replacement = repaired.installed_executable(root.path()).unwrap();
        assert_ne!(damaged, replacement);
        assert_eq!(fs::read(damaged).unwrap(), b"corrupt");
        assert_eq!(fs::read(replacement).unwrap(), b"trusted bundle");
    }
    #[test]
    fn bundle_metadata_cannot_escape_the_store() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("current-release.json"),
            serde_json::json!({
                "bundled": { "version": "../outside", "protocol_version": 12,
                    "target": platform_target().unwrap(), "sha256": "0".repeat(64), "size":1 }
            })
            .to_string(),
        )
        .unwrap();
        assert!(load_installed(root.path()).is_err());
    }
}
