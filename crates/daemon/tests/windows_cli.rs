#![cfg(windows)]

use std::process::Command;

// The GUI subsystem must not break the short-lived CLI commands used by Tauri.
#[test]
fn gui_subsystem_preserves_redirected_cli_output_and_exit_codes() {
    let executable = env!("CARGO_BIN_EXE_amarcode-daemon");
    let help = Command::new(executable).arg("--help").output().unwrap();
    assert!(help.status.success());
    let stdout = String::from_utf8(help.stdout).unwrap();
    for command in ["install", "start", "stop", "restart", "status"] {
        assert!(stdout.contains(command), "missing lifecycle command: {command}");
    }

    let invalid = Command::new(executable)
        .arg("--invalid-amarcode-option")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(!invalid.stderr.is_empty());
}
