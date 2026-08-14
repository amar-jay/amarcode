//! Guarded removal of daemon-owned user data.
//!
//! Cleanup deliberately accepts both a requested and an expected path. The
//! executable passes the configured app directory as the request and the
//! platform default as the expected value. Refusing mismatches prevents
//! `AMARCODE_APPDIR` from becoming an arbitrary recursive-delete target.

use std::{fs, path::Path};

use crate::{
    app_dir,
    service_control::{ServiceController, ServiceState},
    Error, Result,
};

/// Stop and unregister the native service, verify its absence, then remove the
/// platform-owned daemon data directory. The operation is idempotent.
pub fn purge_default<C: ServiceController>(controller: &C) -> Result<()> {
    let requested = app_dir::resolve()?;
    let expected = app_dir::resolve_default()?;
    purge(controller, &requested, &expected)
}

fn purge<C: ServiceController>(
    controller: &C,
    requested_app_dir: &Path,
    expected_app_dir: &Path,
) -> Result<()> {
    validate_target(requested_app_dir, expected_app_dir)?;

    controller.uninstall()?;
    let status = controller.status()?;
    if status.installed || status.state != ServiceState::NotInstalled {
        return Err(Error::msg(
            "refusing to delete daemon data because the native service is still registered",
        ));
    }

    remove_app_dir(requested_app_dir)
}

fn validate_target(requested: &Path, expected: &Path) -> Result<()> {
    if !requested.is_absolute() || !expected.is_absolute() {
        return Err(Error::msg("daemon purge requires an absolute data path"));
    }
    if requested != expected {
        return Err(Error::msg(format!(
            "refusing to purge non-default daemon data directory {}; unset AMARCODE_APPDIR and retry, or remove the override manually",
            requested.display()
        )));
    }
    let parent = requested
        .parent()
        .filter(|parent| parent.parent().is_some())
        .ok_or_else(|| Error::msg("refusing to purge a filesystem root or shallow root child"))?;
    if requested.file_name().is_none() {
        return Err(Error::msg("refusing to purge a filesystem root"));
    }

    if let Ok(metadata) = fs::symlink_metadata(requested) {
        if metadata.file_type().is_symlink() {
            return Err(Error::msg(format!(
                "refusing to purge symlinked daemon data directory {}",
                requested.display()
            )));
        }

        let resolved_target = requested.canonicalize().map_err(|error| {
            Error::msg(format!(
                "failed to validate daemon data directory {}: {error}",
                requested.display()
            ))
        })?;
        let resolved_parent = parent.canonicalize().map_err(|error| {
            Error::msg(format!(
                "failed to validate daemon data parent {}: {error}",
                parent.display()
            ))
        })?;
        if resolved_target.parent() != Some(resolved_parent.as_path())
            || resolved_target.file_name() != requested.file_name()
        {
            return Err(Error::msg(format!(
                "refusing to purge redirected daemon data directory {}",
                requested.display()
            )));
        }
    }

    Ok(())
}

fn remove_app_dir(app_dir: &Path) -> Result<()> {
    match fs::remove_dir_all(app_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::msg(format!(
            "failed to remove daemon data directory {}: {error}",
            app_dir.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        fs,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use crate::service_control::{ServiceController, ServiceState, ServiceStatus};

    use super::purge;

    struct FakeController {
        installed_after_uninstall: bool,
        fail_uninstall: bool,
        events: Mutex<Vec<&'static str>>,
    }

    impl FakeController {
        fn successful() -> Self {
            Self {
                installed_after_uninstall: false,
                fail_uninstall: false,
                events: Mutex::new(Vec::new()),
            }
        }
    }

    impl ServiceController for FakeController {
        fn install(&self, _: &Path, _: Option<&OsStr>) -> crate::Result<()> {
            unreachable!()
        }

        fn uninstall(&self) -> crate::Result<()> {
            self.events.lock().unwrap().push("uninstall");
            if self.fail_uninstall {
                Err(crate::Error::msg("uninstall failed"))
            } else {
                Ok(())
            }
        }

        fn start(&self) -> crate::Result<()> {
            unreachable!()
        }

        fn stop(&self) -> crate::Result<()> {
            unreachable!()
        }

        fn restart(&self) -> crate::Result<()> {
            unreachable!()
        }

        fn status(&self) -> crate::Result<ServiceStatus> {
            self.events.lock().unwrap().push("status");
            Ok(ServiceStatus {
                installed: self.installed_after_uninstall,
                state: if self.installed_after_uninstall {
                    ServiceState::Running
                } else {
                    ServiceState::NotInstalled
                },
                definition_path: None,
            })
        }
    }

    fn test_dir() -> PathBuf {
        std::env::temp_dir()
            .join(format!("amarcode-purge-test-{}", uuid::Uuid::new_v4()))
            .join("amarcode")
    }

    #[test]
    fn unregisters_and_verifies_before_removing_data() {
        let app_dir = test_dir();
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(app_dir.join("workspace.sqlite3"), b"data").unwrap();
        let controller = FakeController::successful();

        purge(&controller, &app_dir, &app_dir).unwrap();

        assert!(!app_dir.exists());
        assert_eq!(*controller.events.lock().unwrap(), ["uninstall", "status"]);
        fs::remove_dir_all(app_dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_data_is_an_idempotent_success() {
        let app_dir = test_dir();
        let controller = FakeController::successful();

        purge(&controller, &app_dir, &app_dir).unwrap();

        assert_eq!(*controller.events.lock().unwrap(), ["uninstall", "status"]);
    }

    #[test]
    fn refuses_an_environment_override_target() {
        let expected = test_dir();
        let requested = expected.parent().unwrap().join("somewhere-else");
        let controller = FakeController::successful();

        let error = purge(&controller, &requested, &expected).unwrap_err();

        assert!(error.to_string().contains("non-default"));
        assert!(controller.events.lock().unwrap().is_empty());
    }

    #[test]
    fn refuses_a_relative_target() {
        let controller = FakeController::successful();
        let relative = Path::new(".amarcode");

        let error = purge(&controller, relative, relative).unwrap_err();

        assert!(error.to_string().contains("absolute"));
        assert!(controller.events.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_filesystem_root() {
        let controller = FakeController::successful();
        let root = Path::new("/");

        let error = purge(&controller, root, root).unwrap_err();

        assert!(error.to_string().contains("filesystem root"));
        assert!(controller.events.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_shallow_root_child() {
        let controller = FakeController::successful();
        let target = Path::new("/.amarcode");

        let error = purge(&controller, target, target).unwrap_err();

        assert!(error.to_string().contains("shallow root child"));
        assert!(controller.events.lock().unwrap().is_empty());
    }

    #[test]
    fn preserves_data_when_service_removal_fails() {
        let app_dir = test_dir();
        fs::create_dir_all(&app_dir).unwrap();
        let controller = FakeController {
            fail_uninstall: true,
            ..FakeController::successful()
        };

        assert!(purge(&controller, &app_dir, &app_dir).is_err());
        assert!(app_dir.is_dir());

        fs::remove_dir_all(app_dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn preserves_data_when_service_is_still_registered() {
        let app_dir = test_dir();
        fs::create_dir_all(&app_dir).unwrap();
        let controller = FakeController {
            installed_after_uninstall: true,
            ..FakeController::successful()
        };

        assert!(purge(&controller, &app_dir, &app_dir).is_err());
        assert!(app_dir.is_dir());

        fs::remove_dir_all(app_dir.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_target() {
        use std::os::unix::fs::symlink;

        let app_dir = test_dir();
        let parent = app_dir.parent().unwrap();
        let real = parent.join("real");
        fs::create_dir_all(&real).unwrap();
        symlink(&real, &app_dir).unwrap();
        let controller = FakeController::successful();

        let error = purge(&controller, &app_dir, &app_dir).unwrap_err();

        assert!(error.to_string().contains("symlinked"));
        assert!(real.is_dir());
        fs::remove_file(&app_dir).unwrap();
        fs::remove_dir_all(parent).unwrap();
    }
}
