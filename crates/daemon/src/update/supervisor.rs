//! Runs without App, SQLite, or an instance lock. Its native service/task is
//! independent of the daemon's service and resumes pending jobs after login.
use super::releases;
use super::{
    compatible,
    journal::{self, Phase, Record},
};
use crate::service_control::{NativeServiceController, ServiceController, ServiceState};
use amarcode_protocol::rpc::{DaemonUpdateStatus, HealthResult};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::Path,
    time::{Duration, Instant},
};

pub trait Host {
    fn activate(&self, executable: &Path) -> Result<(), String>;
    fn health(&self, address: &str, expected: &str) -> Result<HealthResult, String>;
}
pub struct NativeHost;
impl Host for NativeHost {
    fn activate(&self, executable: &Path) -> Result<(), String> {
        let service = NativeServiceController;
        service
            .install(executable, std::env::var_os("PATH").as_deref())
            .map_err(|e| e.to_string())?;
        service.restart().map_err(|e| e.to_string())
    }
    fn health(&self, address: &str, expected: &str) -> Result<HealthResult, String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut error = "daemon did not become healthy".to_string();
        while Instant::now() < deadline {
            match health_once(address, expected) {
                Ok(health) => {
                    let status = NativeServiceController
                        .status()
                        .map_err(|e| e.to_string())?;
                    if status.installed && status.state == ServiceState::Running {
                        return Ok(health);
                    }
                    error = "daemon answered but its native service is not running".into();
                }
                Err(e) => error = e,
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        Err(error)
    }
}
fn health_once(address: &str, expected: &str) -> Result<HealthResult, String> {
    let address = address
        .parse()
        .map_err(|_| "invalid daemon health address")?;
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(1)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(b"{\"method\":\"health\"}\n")
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    // Health replies are small. Never accept an unbounded response here.
    std::io::Read::take(BufReader::new(stream), 64 * 1024)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    let health: HealthResult =
        serde_json::from_value(value["result"].clone()).map_err(|e| e.to_string())?;
    if health.status != "ok"
        || health.version != expected
        || health.protocol_version != amarcode_protocol::PROTOCOL_VERSION
    {
        return Err("replacement daemon failed its exact version/protocol health check".into());
    }
    Ok(health)
}

pub async fn run(root: &Path) -> Result<(), String> {
    let _supervisor = journal::lock(root, "supervisor.lock")?;
    let Some(mut record) = journal::load(root, None)? else {
        return Ok(());
    };
    if record.job.finished {
        return Ok(());
    }
    // Native operations use this independent process's blocking pool.
    let outcome = if matches!(
        record.phase,
        Phase::Installing | Phase::Restarting | Phase::RollingBack
    ) {
        tokio::task::block_in_place(|| recover(root, &mut record, &NativeHost))
    } else {
        match stage(root, &mut record).await {
            Ok(executable) => tokio::task::block_in_place(|| {
                activate(root, &mut record, &executable, &NativeHost)
            }),
            Err(error) => record.finish(root, DaemonUpdateStatus::Failed { error }),
        }
    };
    match outcome {
        Ok(()) => Ok(()),
        Err(error) => recovery_error(root, &mut record, error),
    }
}
fn recovery_error(root: &Path, record: &mut Record, error: String) -> Result<(), String> {
    record.recovery_failures += 1;
    record.last_error = Some(error.clone());
    if record.recovery_failures >= 3 {
        // Bounded recovery permits the desktop's explicit repair action instead
        // of leaving an irrecoverable job pending forever.
        record.finish(root, DaemonUpdateStatus::Failed { error: format!("daemon update recovery failed after three attempts: {error}. Repair the daemon service from the connection dialog") })
    } else {
        record.save(root)?;
        Err(error)
    }
}

async fn stage(root: &Path, record: &mut Record) -> Result<std::path::PathBuf, String> {
    let target = releases::load_cached(&record.directory(root).join("target"))?;
    compatible(&target)?;
    if target.manifest.version != record.job.version {
        return Err("update target changed".into());
    }
    ManagedState::new(root, record).previous_executable()?;
    let artifact = target.artifact(releases::platform_target()?)?;
    record.progress(
        root,
        Phase::Staging,
        DaemonUpdateStatus::Downloading {
            received: 0,
            total: artifact.size,
        },
    )?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?;
    let executable = target
        .ensure_installed(&client, root, releases::platform_target()?)
        .await?;
    record.progress(root, Phase::Staging, DaemonUpdateStatus::Verifying)?;
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(&executable)
            .arg("--help")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "replacement CLI probe timed out")?
    .map_err(|e| e.to_string())?;
    let help = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !help.contains("update-supervisor") {
        return Err("replacement lacks daemon update supervision support".into());
    }
    Ok(executable)
}
/// Release storage is separate from service control so transaction ordering and
/// crash recovery can be tested without touching the user's service manager.
trait ReleaseState {
    fn commit_target(&self) -> Result<(), String>;
    fn previous_executable(&self) -> Result<std::path::PathBuf, String>;
    fn commit_previous(&self) -> Result<(), String>;
    fn target_committed(&self) -> bool;
}
struct ManagedState<'a> {
    root: &'a Path,
    directory: std::path::PathBuf,
    previous: String,
    target: String,
}
impl<'a> ManagedState<'a> {
    fn new(root: &'a Path, record: &Record) -> Self {
        Self {
            root,
            directory: record.directory(root),
            previous: record.job.previous_version.clone(),
            target: record.job.version.clone(),
        }
    }
    fn release(&self, previous: bool) -> Result<releases::InstalledRelease, String> {
        let path = self
            .directory
            .join(if previous { "previous" } else { "target" });
        let manifest = if previous {
            releases::load_installed(&path)?
        } else {
            // Network targets always require a signed manifest.
            releases::InstalledRelease::Signed(releases::load_cached(&path)?)
        };
        super::compatible_installed(&manifest)?;
        if manifest.version()
            != if previous {
                &self.previous
            } else {
                &self.target
            }
        {
            return Err("pinned update release changed".into());
        }
        Ok(manifest)
    }
}
impl ReleaseState for ManagedState<'_> {
    fn commit_target(&self) -> Result<(), String> {
        self.release(false)?.save_as_current(self.root)
    }
    fn previous_executable(&self) -> Result<std::path::PathBuf, String> {
        self.release(true)?.installed_executable(self.root)
    }
    fn commit_previous(&self) -> Result<(), String> {
        self.release(true)?.save_as_current(self.root)
    }
    fn target_committed(&self) -> bool {
        let result = (|| {
            let current = releases::load_cached(self.root)?;
            let target = releases::load_cached(&self.directory.join("target"))?;
            compatible(&target)?;
            let platform = releases::platform_target()?;
            Ok::<_, String>(
                current.manifest.version == self.target
                    && current.manifest.protocol_version == target.manifest.protocol_version
                    && current.artifact(platform)?.sha256 == target.artifact(platform)?.sha256,
            )
        })();
        result.unwrap_or(false)
    }
}
fn activate(
    root: &Path,
    record: &mut Record,
    executable: &Path,
    host: &impl Host,
) -> Result<(), String> {
    let state = ManagedState::new(root, record);
    activate_with(root, record, executable, host, &state)
}
fn activate_with(
    root: &Path,
    record: &mut Record,
    executable: &Path,
    host: &impl Host,
    state: &impl ReleaseState,
) -> Result<(), String> {
    record.progress(root, Phase::Installing, DaemonUpdateStatus::Installing)?;
    let outcome = (|| {
        host.activate(executable)?;
        record.progress(root, Phase::Restarting, DaemonUpdateStatus::Restarting)?;
        host.health(&record.address, &record.job.version)?;
        state.commit_target()?;
        Ok::<(), String>(())
    })();
    match outcome {
        Ok(()) => record.finish(
            root,
            DaemonUpdateStatus::Ready {
                version: record.job.version.clone(),
            },
        ),
        Err(error) => rollback_with(root, record, host, state, error),
    }
}
fn recover(root: &Path, record: &mut Record, host: &impl Host) -> Result<(), String> {
    let state = ManagedState::new(root, record);
    recover_with(root, record, host, &state)
}
fn recover_with(
    root: &Path,
    record: &mut Record,
    host: &impl Host,
    state: &impl ReleaseState,
) -> Result<(), String> {
    // The atomic signed release record is the commit point, not the job phase.
    if state.target_committed() && host.health(&record.address, &record.job.version).is_ok() {
        return record.finish(
            root,
            DaemonUpdateStatus::Ready {
                version: record.job.version.clone(),
            },
        );
    }
    rollback_with(
        root,
        record,
        host,
        state,
        "update activation was interrupted".into(),
    )
}
fn rollback_with(
    root: &Path,
    record: &mut Record,
    host: &impl Host,
    state: &impl ReleaseState,
    error: String,
) -> Result<(), String> {
    record.progress(root, Phase::RollingBack, DaemonUpdateStatus::RollingBack)?;
    let executable = state.previous_executable()?;
    // On failure leave RollingBack durable and exit unsuccessfully. The OS
    // retries this independent supervisor, including after the next login.
    host.activate(&executable)?;
    host.health(&record.address, &record.job.previous_version)?;
    state.commit_previous()?;
    record.finish(
        root,
        DaemonUpdateStatus::Failed {
            error: format!("{error}; previous daemon restored"),
        },
    )
}

#[cfg(test)]
mod bundled_rollback_tests {
    use super::*;
    use std::cell::RefCell;
    struct FakeHost(RefCell<Vec<std::path::PathBuf>>);
    impl Host for FakeHost {
        fn activate(&self, executable: &Path) -> Result<(), String> {
            self.0.borrow_mut().push(executable.into());
            Ok(())
        }
        fn health(&self, address: &str, expected: &str) -> Result<HealthResult, String> {
            Ok(HealthResult {
                status: "ok".into(),
                version: expected.into(),
                protocol_version: amarcode_protocol::PROTOCOL_VERSION,
                addr: address.into(),
            })
        }
    }
    fn record() -> Record {
        Record {
            job: amarcode_protocol::rpc::DaemonUpdateJob {
                id: uuid::Uuid::new_v4().to_string(),
                version: "99.0.0".into(),
                previous_version: env!("CARGO_PKG_VERSION").into(),
                status: DaemonUpdateStatus::Restarting,
                finished: false,
            },
            phase: Phase::Restarting,
            address: "127.0.0.1:43821".into(),
            recovery_failures: 0,
            last_error: None,
        }
    }
    #[test]
    fn downloaded_update_can_roll_back_to_the_original_desktop_bundle() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("desktop-bundle");
        std::fs::write(&source, b"verified bundled fixture").unwrap();
        let previous = releases::stage_bundle_from(root.path(), &source).unwrap();
        let mut record = record();
        previous
            .save_as_current(&record.directory(root.path()).join("previous"))
            .unwrap();
        std::fs::remove_file(source).unwrap();
        let state = ManagedState::new(root.path(), &record);
        let host = FakeHost(RefCell::new(Vec::new()));
        rollback_with(
            root.path(),
            &mut record,
            &host,
            &state,
            "new release failed health".into(),
        )
        .unwrap();
        assert!(record.job.finished);
        assert!(
            matches!(&record.job.status, DaemonUpdateStatus::Failed { error } if error.contains("previous daemon restored"))
        );
        let restored = releases::load_installed(root.path())
            .unwrap()
            .installed_executable(root.path())
            .unwrap();
        assert_eq!(*host.0.borrow(), [restored]);
    }
    #[test]
    fn a_bundle_record_cannot_be_used_as_a_downloaded_update_target() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("desktop-bundle");
        std::fs::write(&source, b"bundled fixture").unwrap();
        let bundle = releases::stage_bundle_from(root.path(), &source).unwrap();
        let record = record();
        bundle
            .save_as_current(&record.directory(root.path()).join("target"))
            .unwrap();
        assert!(ManagedState::new(root.path(), &record)
            .release(false)
            .is_err());
    }
}
