//! The supervisor CLI must never construct App or acquire its database lock.
use serde_json::json;
use std::{fs, process::Command};
#[test]
fn supervisor_is_independent_of_daemon_configuration_and_database() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("must-not-exist");
    let output = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"))
        .args(["update-supervisor", "--release-root"])
        .arg(root.path())
        .env("AMARCODE_APPDIR", &database)
        .env("AMARCODE_DAEMON_ADDR", "invalid-address")
        .env("AMARCODE_ACP_REGISTRY_SOURCE", "invalid-source")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!database.exists());
    let id = uuid::Uuid::new_v4().to_string();
    fs::create_dir_all(root.path().join("updates").join(&id)).unwrap();
    fs::write(root.path().join("updates/latest"), &id).unwrap();
    fs::write(root.path().join("updates").join(&id).join("job.json"), json!({
        "job": { "id": id, "version": "9.0.0", "previous_version": "1.0.0", "status": {"status":"ready","version":"9.0.0"}, "finished":true },
        "phase":"Finished", "address":"127.0.0.1:43821"
    }).to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"))
        .args(["update-supervisor", "--release-root"])
        .arg(root.path())
        .env("AMARCODE_APPDIR", &database)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!database.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn update_rpc_reports_unavailable_for_custom_services_without_touching_registration() {
    use amarcode_daemon::{
        rpc::handler::{dispatch, DispatchOutcome},
        App, Config,
    };
    let root = tempfile::tempdir().unwrap();
    let app = App::new(Config {
        app_dir: root.path().to_owned(),
        db_path: root.path().join("workspace.sqlite3"),
        daemon_addr: "127.0.0.1:0".into(),
        transport: Default::default(),
        ws_tls_cert: None,
        ws_tls_key: None,
        ws_allowed_origins: Vec::new(),
        acp_registry_source: None,
    })
    .await
    .unwrap();
    let result = dispatch(&app, "check_daemon_update", json!({}))
        .await
        .unwrap();
    assert!(matches!(result, DispatchOutcome::Result(value) if value["status"] == "unavailable"));
    let error = dispatch(&app, "start_daemon_update", json!({"version":"99.0.0"}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("standard managed desktop"));
    let error = dispatch(&app, "get_daemon_update", json!({"job_id":"../escape"}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("job id"));
}
