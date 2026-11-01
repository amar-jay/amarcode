#![cfg(target_os = "linux")]
//! A real local X server and CLI PTY verify cookie substitution end to end.
#[test]
fn forwards_x11_windows_and_cleans_up_on_exit_and_disconnect() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/fixtures/x11-forwarding.py");
    let output = std::process::Command::new("python3")
        .arg(fixture)
        .arg(env!("CARGO_BIN_EXE_amarcode-daemon"))
        .output()
        .expect("start X11 fixture");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
