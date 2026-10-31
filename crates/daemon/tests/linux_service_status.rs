#![cfg(target_os = "linux")]
//! A container's systemctl shim must never masquerade as a running daemon.
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn status_requires_an_actual_active_systemd_state() {
    for (output, exit, expected) in [
        ("active", 0, "running"),
        ("inactive", 3, "stopped"),
        ("failed", 3, "stopped"),
        ("activating", 3, "unknown"),
        ("systemd is not running in this container", 0, "unknown"),
        ("", 1, "unknown"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let units = root.path().join("config/systemd/user");
        fs::create_dir_all(&units).unwrap();
        fs::write(units.join("amarcode-daemon.service"), "fixture").unwrap();
        let executable = root.path().join("systemctl");
        fs::write(
            &executable,
            format!("#!/bin/sh\nprintf '%s\\n' '{output}'\nexit {exit}\n"),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"))
            .args(["status", "--json"])
            .env("PATH", root.path())
            .env("HOME", root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(status["installed"], true);
        assert_eq!(status["state"], expected, "{output}");
    }
}
