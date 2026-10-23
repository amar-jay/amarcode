//! Short-lived desktop bootstrap commands. No App, database, or RPC instance
//! lock is constructed here. First installation uses the desktop's bundled
//! executable and never requires the release endpoint.
use super::{
    journal, releases,
    supervisor::{Host, NativeHost},
};
use crate::{
    config::DEFAULT_DAEMON_ADDR,
    service_control::{NativeServiceController, ServiceController, ServiceState},
};
use amarcode_protocol::rpc::{DaemonBootstrapEvent, DaemonBootstrapStatus, HealthResult};
use clap::Subcommand;
use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

const INSTALLED: &str = "service-installed-v1";
const ACTIVATING: &str = "bootstrap-activation-pending-v1";
const LEGACY_ACTIVATING: &str = "update-activation-pending-v1";

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum Command {
    /// Check/start an existing managed service. Never performs first installation.
    Ensure,
    /// Install or repair from the bundled executable, including offline.
    Install,
    /// Signed update check for an older daemon without update RPC.
    CheckUpdate,
    /// Hand an approved update to an independent supervisor for an older daemon.
    #[command(disable_version_flag = true)]
    StartUpdate {
        #[arg(long)]
        version: String,
    },
    /// Read a durable job while the RPC listener is restarting.
    UpdateStatus {
        #[arg(long)]
        job_id: Option<String>,
    },
}

fn output(value: &impl serde::Serialize) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value).map_err(|e| e.to_string())?;
    writeln!(stdout)
        .and_then(|_| stdout.flush())
        .map_err(|e| e.to_string())
}

pub async fn run(command: Command) -> Result<(), String> {
    let root = super::release_root()?;
    match command {
        Command::Ensure | Command::Install => {
            let install = command == Command::Install;
            let health = tokio::task::block_in_place(|| {
                let progress = |status| output(&DaemonBootstrapEvent::Progress { status });
                if install {
                    install_with(&root, &NativeHost, &progress).map(Some)
                } else {
                    ensure(&root, &progress)
                }
            })?;
            output(&DaemonBootstrapEvent::Complete { health })
        }
        Command::CheckUpdate => {
            let current = releases::load_installed(&root)?;
            super::compatible_installed(&current)?;
            current.installed_executable(&root)?;
            let latest = releases::fetch_latest(&super::client()?).await?;
            super::compatible(&latest)?;
            output(&super::compare(
                current.version(),
                &latest.manifest.version,
            )?)
        }
        Command::StartUpdate { version } => {
            // The old executable may predate update-supervisor. Run the
            // supervisor from a stable copy of this desktop's bundled daemon.
            let owned = root.clone();
            let (previous, supervisor) = tokio::task::spawn_blocking(move || {
                let previous = releases::load_installed(&owned)?;
                super::compatible_installed(&previous)?;
                previous.installed_executable(&owned)?;
                NativeHost.health(DEFAULT_DAEMON_ADDR, previous.version())?;
                let bundle = releases::stage_bundle(&owned)?;
                Ok::<_, String>((previous, bundle.installed_executable(&owned)?))
            })
            .await
            .map_err(|e| e.to_string())??;
            let job = super::start_managed(
                root,
                previous,
                supervisor,
                DEFAULT_DAEMON_ADDR.into(),
                version,
            )
            .await?;
            output(&job)
        }
        Command::UpdateStatus { job_id } => {
            output(&releases::read_update_job(&root, job_id.as_deref())?)
        }
    }
}

fn ensure(
    root: &Path,
    progress: &impl Fn(DaemonBootstrapStatus) -> Result<(), String>,
) -> Result<Option<HealthResult>, String> {
    progress(DaemonBootstrapStatus::Checking)?;
    let pending = {
        let _request = journal::lock(root, "request.lock")?;
        match journal::load(root, None)? {
            Some(record) if !record.job.finished => {
                // An older daemon may lack update-supervisor; use a stable
                // copy of the bundled CLI for transition recovery as well.
                let supervisor = releases::stage_bundle(root)?.installed_executable(root)?;
                super::launch(&supervisor, root, false)?;
                Some(record.job.id)
            }
            _ => None,
        }
    };
    if let Some(id) = pending {
        progress(DaemonBootstrapStatus::Starting)?;
        wait_for_job(root, &id)?;
    }
    let _request = journal::lock(root, "request.lock")?;
    let _supervisor = journal::lock(root, "supervisor.lock")?;
    if root.join(ACTIVATING).exists() {
        recover(root, &NativeHost)?;
    }
    if root.join(LEGACY_ACTIVATING).exists() {
        let current = releases::load_installed(root)?;
        activate(&NativeHost, root, &current)?;
        std::fs::remove_file(root.join(LEGACY_ACTIVATING)).map_err(|e| e.to_string())?;
    }
    if !root.join(INSTALLED).is_file() {
        progress(DaemonBootstrapStatus::InstallRequired {
            reason: "The per-user background service has not been installed yet.".into(),
        })?;
        return Ok(None);
    }
    let current = match releases::load_installed(root).and_then(|current| {
        super::compatible_installed(&current)?;
        current.installed_executable(root)?;
        Ok(current)
    }) {
        Ok(current) => current,
        Err(_) => {
            progress(DaemonBootstrapStatus::InstallRequired {
                reason: "The background service needs repair from the bundled daemon.".into(),
            })?;
            return Ok(None);
        }
    };
    let status = NativeServiceController
        .status()
        .map_err(|e| e.to_string())?;
    if !status.installed {
        progress(DaemonBootstrapStatus::InstallRequired {
            reason: "The per-user background service is no longer registered.".into(),
        })?;
        return Ok(None);
    }
    if status.state != ServiceState::Running {
        progress(DaemonBootstrapStatus::Starting)?;
        NativeServiceController.start().map_err(|e| e.to_string())?;
    }
    let health = NativeHost.health(DEFAULT_DAEMON_ADDR, current.version())?;
    progress(DaemonBootstrapStatus::Ready {
        version: health.version.clone(),
    })?;
    Ok(Some(health))
}

fn wait_for_job(root: &Path, id: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        let record = journal::load(root, Some(id))?.ok_or("daemon update job disappeared")?;
        if record.job.finished {
            // After a failed update, the committed previous release can still
            // start normally. Explicit repair handles unrecoverable rollbacks.
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("daemon update is still running independently; retry shortly".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn install_with(
    root: &Path,
    host: &impl Host,
    progress: &impl Fn(DaemonBootstrapStatus) -> Result<(), String>,
) -> Result<HealthResult, String> {
    install_with_bundle(root, host, progress, &|| releases::stage_bundle(root))
}

fn install_with_bundle(
    root: &Path,
    host: &impl Host,
    progress: &impl Fn(DaemonBootstrapStatus) -> Result<(), String>,
    bundled: &impl Fn() -> Result<releases::InstalledRelease, String>,
) -> Result<HealthResult, String> {
    let _request = journal::lock(root, "request.lock")?;
    let _supervisor = journal::lock(root, "supervisor.lock")?;
    if journal::load(root, None)?.is_some_and(|record| !record.job.finished) {
        return Err("a daemon update is active; wait for it to finish before repairing".into());
    }
    if root.join(ACTIVATING).exists() {
        // Explicit repair must also work when the interrupted target itself is
        // damaged. A new transaction below replaces that stale handoff safely.
        let _ = recover(root, host);
    }
    progress(DaemonBootstrapStatus::Verifying)?;
    let previous = releases::load_installed(root).ok().filter(|release| {
        super::compatible_installed(release).is_ok() && release.installed_executable(root).is_ok()
    });
    // Preserve a valid newer downloaded release during desktop repair. Otherwise
    // use this package's offline bundle, never silently downgrade to a download.
    let target = match &previous {
        Some(current)
            if semver::Version::parse(current.version()).ok()
                > semver::Version::parse(env!("CARGO_PKG_VERSION")).ok() =>
        {
            current.clone()
        }
        _ => bundled()?,
    };
    install_release(root, host, progress, previous.as_ref(), &target)
}

fn install_release(
    root: &Path,
    host: &impl Host,
    progress: &impl Fn(DaemonBootstrapStatus) -> Result<(), String>,
    previous: Option<&releases::InstalledRelease>,
    target: &releases::InstalledRelease,
) -> Result<HealthResult, String> {
    let previous_dir = root.join("bootstrap-previous");
    if previous_dir.exists() {
        std::fs::remove_dir_all(&previous_dir).map_err(|e| e.to_string())?;
    }
    if let Some(previous) = previous {
        previous.save_as_current(&previous_dir)?;
    }
    target.save_as_current(&root.join("bootstrap-target"))?;
    releases::atomic_write(&root.join(ACTIVATING), b"1\n")?;
    progress(DaemonBootstrapStatus::Installing)?;
    progress(DaemonBootstrapStatus::Starting)?;
    match activate(host, root, target) {
        Ok(health) => {
            target.save_as_current(root)?;
            releases::atomic_write(&root.join(INSTALLED), b"1\n")?;
            std::fs::remove_file(root.join(ACTIVATING)).map_err(|e| e.to_string())?;
            progress(DaemonBootstrapStatus::Ready {
                version: health.version.clone(),
            })?;
            Ok(health)
        }
        Err(error) => {
            if let Some(previous) = previous {
                activate(host, root, previous).map_err(|rollback| {
                    format!("{error}; bootstrap rollback failed: {rollback}")
                })?;
                previous.save_as_current(root)?;
                std::fs::remove_file(root.join(ACTIVATING)).map_err(|e| e.to_string())?;
                return Err(format!("{error}; previous daemon restored"));
            }
            Err(error)
        }
    }
}
fn activate(
    host: &impl Host,
    root: &Path,
    release: &releases::InstalledRelease,
) -> Result<HealthResult, String> {
    super::compatible_installed(release)?;
    host.activate(&release.installed_executable(root)?)?;
    host.health(DEFAULT_DAEMON_ADDR, release.version())
}
fn recover(root: &Path, host: &impl Host) -> Result<(), String> {
    let target = releases::load_installed(&root.join("bootstrap-target"))?;
    match activate(host, root, &target) {
        Ok(_) => {
            target.save_as_current(root)?;
            releases::atomic_write(&root.join(INSTALLED), b"1\n")?;
        }
        Err(error) => {
            let previous = releases::load_installed(&root.join("bootstrap-previous"))
                .map_err(|_| format!("{error}; interrupted first installation needs repair"))?;
            activate(host, root, &previous)?;
            previous.save_as_current(root)?;
        }
    }
    std::fs::remove_file(root.join(ACTIVATING)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        path::PathBuf,
    };
    struct FakeHost {
        activated: RefCell<Vec<PathBuf>>,
        fail_health_once: Cell<bool>,
    }
    impl Host for FakeHost {
        fn activate(&self, executable: &Path) -> Result<(), String> {
            assert!(executable.is_file());
            self.activated.borrow_mut().push(executable.to_owned());
            Ok(())
        }
        fn health(&self, address: &str, expected: &str) -> Result<HealthResult, String> {
            assert_eq!(address, DEFAULT_DAEMON_ADDR);
            if self.fail_health_once.replace(false) {
                return Err("replacement failed health".into());
            }
            Ok(HealthResult {
                status: "ok".into(),
                version: expected.into(),
                protocol_version: amarcode_protocol::PROTOCOL_VERSION,
                addr: address.into(),
            })
        }
    }
    fn host(fail_health_once: bool) -> FakeHost {
        FakeHost {
            activated: RefCell::new(Vec::new()),
            fail_health_once: Cell::new(fail_health_once),
        }
    }
    fn bundle(root: &Path, bytes: &[u8]) -> releases::InstalledRelease {
        let source = root.join("desktop-source");
        std::fs::write(&source, bytes).unwrap();
        let bundle = releases::stage_bundle_from(root, &source).unwrap();
        std::fs::remove_file(source).unwrap();
        bundle
    }
    #[test]
    fn first_install_and_repair_work_offline_without_desktop_source_or_downloads() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let target = bundle(root, b"bundled executable");
        let host = host(false);
        let events = RefCell::new(Vec::new());
        let progress = |status| {
            events.borrow_mut().push(status);
            Ok(())
        };
        install_with_bundle(root, &host, &progress, &|| Ok(target.clone())).unwrap();
        assert!(root.join(INSTALLED).is_file());
        assert!(!root.join(ACTIVATING).exists());
        assert_eq!(
            releases::load_installed(root)
                .unwrap()
                .installed_executable(root)
                .unwrap(),
            target.installed_executable(root).unwrap()
        );
        assert_eq!(host.activated.borrow().len(), 1);
        assert!(matches!(
            events.borrow().last(),
            Some(DaemonBootstrapStatus::Ready { .. })
        ));
        // Repair reuses the verified installed copy, without a release endpoint.
        install_with_bundle(root, &host, &progress, &|| Ok(target.clone())).unwrap();
        assert_eq!(host.activated.borrow().len(), 2);
    }
    #[test]
    fn failed_install_restores_the_previous_verified_executable() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let previous = bundle(root, b"previous desktop bundle");
        previous.save_as_current(root).unwrap();
        let target = bundle(root, b"new desktop bundle");
        let host = host(true);
        let error =
            install_release(root, &host, &|_| Ok(()), Some(&previous), &target).unwrap_err();
        assert!(error.contains("previous daemon restored"));
        assert_eq!(
            *host.activated.borrow(),
            [
                target.installed_executable(root).unwrap(),
                previous.installed_executable(root).unwrap()
            ]
        );
        assert_eq!(
            releases::load_installed(root)
                .unwrap()
                .installed_executable(root)
                .unwrap(),
            previous.installed_executable(root).unwrap()
        );
        assert!(!root.join(ACTIVATING).exists());
    }
    #[test]
    fn interrupted_first_install_finishes_from_its_durable_bundle_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let target = bundle(root, b"bundled executable");
        target
            .save_as_current(&root.join("bootstrap-target"))
            .unwrap();
        releases::atomic_write(&root.join(ACTIVATING), b"1").unwrap();
        recover(root, &host(false)).unwrap();
        assert!(root.join(INSTALLED).exists());
        assert!(!root.join(ACTIVATING).exists());
        assert_eq!(
            releases::load_installed(root).unwrap().version(),
            target.version()
        );
    }
    #[test]
    fn ensure_never_performs_first_installation() {
        let directory = tempfile::tempdir().unwrap();
        let statuses = RefCell::new(Vec::new());
        assert!(ensure(directory.path(), &|status| {
            statuses.borrow_mut().push(status);
            Ok(())
        })
        .unwrap()
        .is_none());
        assert!(matches!(
            statuses.borrow().last(),
            Some(DaemonBootstrapStatus::InstallRequired { .. })
        ));
        assert!(!directory.path().join(INSTALLED).exists());
        assert!(!directory.path().join("bundled").exists());
    }
}
