//! Real SIGTERM regressions for the maintenance jobs that formerly held the
//! Tokio runtime open after the daemon logged "shutdown complete".
#![cfg(unix)]
use amarcode_daemon::{protocol::AgentDefinition, store::Store};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn executable(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn agent(store: &Store, id: &str, command: &Path) {
    store
        .save_agent(&AgentDefinition {
            id: id.into(),
            name: id.into(),
            command: command.to_string_lossy().into(),
            arguments: vec![],
            environment: vec![],
            available: true,
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
        })
        .unwrap();
}
struct Daemon {
    child: Child,
    app: PathBuf,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            if let Ok(pid) = std::fs::read_to_string(self.app.join("install-pid")) {
                if let Ok(pid) = pid.trim().parse() {
                    let _ = nix::sys::signal::killpg(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn command(root: &Path, app: &Path) -> (Command, String) {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap().to_string();
    let mut command = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    command
        .arg("run")
        .env("AMARCODE_APPDIR", app)
        .env("XDG_DATA_HOME", root.join("state"))
        .env("AMARCODE_DAEMON_ADDR", &address)
        .env("AMARCODE_ACP_REGISTRY_SOURCE", "")
        .env("AMARCODE_WORKLOAD_ISOLATION", "none")
        .env_remove("AMARCODE_STORE_PATH")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    (command, address)
}
fn ready(daemon: &mut Daemon, address: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            daemon.child.try_wait().unwrap().is_none(),
            "daemon exited during startup"
        );
        if let Ok(mut stream) = TcpStream::connect(address) {
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            stream.write_all(b"{\"method\":\"health\"}\n").unwrap();
            let mut reply = String::new();
            if BufReader::new(stream).read_line(&mut reply).is_ok() && reply.contains("\"ok\"") {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "daemon health did not become ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn stop(daemon: &mut Daemon) {
    let started = Instant::now();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(daemon.child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    loop {
        if let Some(status) = daemon.child.try_wait().unwrap() {
            assert!(status.success(), "daemon failed during shutdown: {status}");
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "SIGTERM shutdown stalled"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn sigterm_cancels_builtin_manifest_download_headers_and_body() {
    for body in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("data");
        std::fs::create_dir_all(&app).unwrap();
        let installed = root.path().join("installed-acp");
        executable(&installed, "#!/bin/sh\nexit 0\n");
        agent(
            &Store::open(&app.join("workspace.sqlite3")).unwrap(),
            "amarcode-acp",
            &installed,
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
        let (received, request) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            if body {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n{")
                    .unwrap();
            }
            received.send(()).unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(10));
        });
        let (mut command, address) = command(root.path(), &app);
        command.env("AMARCODE_ACP_RELEASE_URL", url);
        let mut daemon = Daemon {
            child: command.spawn().unwrap(),
            app: app.clone(),
        };
        ready(&mut daemon, &address);
        request.recv_timeout(Duration::from_secs(5)).unwrap();
        stop(&mut daemon);
        release.send(()).unwrap();
        server.join().unwrap();
        let saved = Store::open(&app.join("workspace.sqlite3"))
            .unwrap()
            .get_agent("amarcode-acp")
            .unwrap()
            .unwrap();
        assert_eq!(saved.command, installed.to_string_lossy());
        assert!(installed.is_file());
    }
}

#[test]
fn sigterm_kills_installer_process_tree_and_does_not_start_the_next_update() {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("data");
    let registry = app.join("acp-registry");
    std::fs::create_dir_all(&registry).unwrap();
    for schema in ["agent.schema.json", "registry.schema.json"] {
        std::fs::write(registry.join(schema), "{}").unwrap();
    }
    let installed = root.path().join("old-agent");
    executable(&installed, "#!/bin/sh\nexit 0\n");
    let store = Store::open(&app.join("workspace.sqlite3")).unwrap();
    for id in ["slow-agent-a", "slow-agent-b"] {
        agent(&store, id, &installed);
        let folder = registry.join(id);
        std::fs::create_dir(&folder).unwrap();
        let target = amarcode_daemon::registry::current_binary_target().unwrap();
        let manifest = serde_json::json!({"id":id,"name":id,"version":"2.0.0","distribution":{"binary":{target:{"archive":"https://unused.invalid/agent.tar.gz","cmd":"./agent"}}}});
        std::fs::write(folder.join("agent.json"), manifest.to_string()).unwrap();
    }
    drop(store);
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    executable(&bin.join("curl"), "#!/bin/sh\necho $$ > \"$AMARCODE_APPDIR/install-pid\"\necho start >> \"$AMARCODE_APPDIR/install-count\"\nsleep 60 &\necho $! > \"$AMARCODE_APPDIR/install-child\"\nwait\n");
    let (mut command, address) = command(root.path(), &app);
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    command.env("PATH", std::env::join_paths(paths).unwrap());
    let mut daemon = Daemon {
        child: command.spawn().unwrap(),
        app: app.clone(),
    };
    ready(&mut daemon, &address);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.join("install-child").exists() {
        assert!(Instant::now() < deadline, "installer never started");
        thread::sleep(Duration::from_millis(10));
    }
    stop(&mut daemon);
    assert_eq!(
        std::fs::read_to_string(app.join("install-count")).unwrap(),
        "start\n"
    );
    #[cfg(target_os = "linux")]
    {
        let pid = std::fs::read_to_string(app.join("install-child")).unwrap();
        let status = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()));
        assert!(
            status.is_err()
                || status
                    .unwrap()
                    .split(')')
                    .nth(1)
                    .unwrap()
                    .trim_start()
                    .starts_with('Z'),
            "installer descendant survived"
        );
    }
    let store = Store::open(&app.join("workspace.sqlite3")).unwrap();
    assert_eq!(
        store.get_agent("slow-agent-a").unwrap().unwrap().command,
        installed.to_string_lossy()
    );
}

#[test]
fn sigterm_unblocks_an_agent_rpc_waiting_for_initialization() {
    blocked_initialization("none");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a systemd user manager and cgroup v2"]
fn sigterm_unblocks_an_isolated_agent_rpc_waiting_for_initialization() {
    blocked_initialization("systemd");
}

fn blocked_initialization(isolation: &str) {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("data");
    std::fs::create_dir_all(&app).unwrap();
    let marker = app.join("agent-started");
    let script = root.path().join("agent.py");
    std::fs::write(&script, "import os,pathlib,sys,time\nsys.stdin.readline()\npathlib.Path(sys.argv[1]).write_text(str(os.getpid()))\ntime.sleep(60)\n").unwrap();
    let store = Store::open(&app.join("workspace.sqlite3")).unwrap();
    agent(&store, "blocked-agent", Path::new("/usr/bin/python3"));
    let mut definition = store.get_agent("blocked-agent").unwrap().unwrap();
    definition.arguments = vec![
        script.to_string_lossy().into(),
        marker.to_string_lossy().into(),
    ];
    store.save_agent(&definition).unwrap();
    drop(store);
    let (mut command, address) = command(root.path(), &app);
    command.env("AMARCODE_WORKLOAD_ISOLATION", isolation);
    let mut daemon = Daemon {
        child: command.spawn().unwrap(),
        app: app.clone(),
    };
    ready(&mut daemon, &address);
    let mut stream = TcpStream::connect(&address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    writeln!(
        stream,
        "{}",
        serde_json::json!({"method":"create_chat","params":{"workspace_path":root.path()}})
    )
    .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut reply = String::new();
    reader.read_line(&mut reply).unwrap();
    let chat: serde_json::Value = serde_json::from_str(&reply).unwrap();
    let chat_id = chat["result"]["id"].as_str().unwrap();
    writeln!(stream, "{}", serde_json::json!({"method":"discover_commands","params":{"chat_id":chat_id,"agent_id":"blocked-agent"}})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "agent did not receive initialize"
        );
        thread::sleep(Duration::from_millis(10));
    }
    stop(&mut daemon);
    let store = Store::open(&app.join("workspace.sqlite3")).unwrap();
    assert_eq!(
        store.list_runs_for_chat(chat_id).unwrap()[0].status,
        amarcode_daemon::protocol::RunStatus::Stopped
    );
}

#[test]
fn sigterm_interrupts_an_active_prompt_and_preserves_queued_followups() {
    fn rpc(address: &str, request: serde_json::Value) -> serde_json::Value {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        writeln!(stream, "{request}").unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("data");
    std::fs::create_dir_all(&app).unwrap();
    let marker = app.join("prompt-started");
    let script = root.path().join("agent.py");
    std::fs::write(
        &script,
        r#"import json,os,pathlib,sys,time
for line in sys.stdin:
    request=json.loads(line)
    if 'id' not in request: continue
    method=request['method']
    if method=='initialize': result={'protocolVersion':1,'agentCapabilities':{}}
    elif method=='session/new': result={'sessionId':'test-session'}
    elif method=='session/prompt':
        pathlib.Path(sys.argv[1]).write_text(str(os.getpid()))
        time.sleep(60)
        result={'stopReason':'end_turn'}
    else: result={}
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}),flush=True)
"#,
    )
    .unwrap();
    let store = Store::open(&app.join("workspace.sqlite3")).unwrap();
    agent(&store, "blocked-agent", Path::new("/usr/bin/python3"));
    let mut definition = store.get_agent("blocked-agent").unwrap().unwrap();
    definition.arguments = vec![
        script.to_string_lossy().into(),
        marker.to_string_lossy().into(),
    ];
    store.save_agent(&definition).unwrap();
    drop(store);
    let (mut command, address) = command(root.path(), &app);
    let mut daemon = Daemon {
        child: command.spawn().unwrap(),
        app: app.clone(),
    };
    ready(&mut daemon, &address);
    let chat = rpc(
        &address,
        serde_json::json!({"method":"create_chat","params":{"workspace_path":root.path()}}),
    );
    let chat_id = chat["result"]["id"].as_str().unwrap();
    let mut prompt = TcpStream::connect(&address).unwrap();
    writeln!(prompt, "{}", serde_json::json!({"method":"prompt","params":{"chat_id":chat_id,"agent_id":"blocked-agent","text":"work"}})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "agent never received prompt");
        thread::sleep(Duration::from_millis(10));
    }
    let queued = rpc(
        &address,
        serde_json::json!({"method":"enqueue_prompt","params":{"request_id":"followup","prompt":{"chat_id":chat_id,"agent_id":"blocked-agent","text":"next"}}}),
    );
    assert!(queued.get("error").is_none(), "{queued}");
    stop(&mut daemon);
    let store = Store::open(&app.join("workspace.sqlite3")).unwrap();
    let queue = store.prompt_queue(chat_id).unwrap();
    assert!(queue.paused);
    assert!(queue
        .items
        .iter()
        .any(|job| job.id == "followup" && job.status == "queued"));
    assert_eq!(
        store.list_runs_for_chat(chat_id).unwrap()[0].status,
        amarcode_daemon::protocol::RunStatus::Stopped
    );
}

#[test]
#[cfg(not(windows))]
fn stalled_registry_does_not_delay_health_or_cached_catalog_and_is_cancelled_on_shutdown() {
    for cached in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("data");
        std::fs::create_dir_all(&app).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let source = format!("http://{}/registry", listener.local_addr().unwrap());
        if cached {
            let checkout = app.join("acp-registry");
            std::fs::create_dir_all(checkout.join("cached-agent")).unwrap();
            assert!(Command::new("git")
                .args(["init", "--quiet"])
                .arg(&checkout)
                .status()
                .unwrap()
                .success());
            assert!(Command::new("git")
                .arg("-C")
                .arg(&checkout)
                .args(["remote", "add", "origin", &source])
                .status()
                .unwrap()
                .success());
            std::fs::write(checkout.join("registry.schema.json"), "{}").unwrap();
            std::fs::write(checkout.join("agent.schema.json"), "{}").unwrap();
            std::fs::write(
                checkout.join("cached-agent/agent.json"),
                r#"{
                "id":"cached-agent", "name":"Cached Agent", "version":"1.0.0",
                "distribution":{"npx":{"package":"cached-agent@1.0.0"}}
            }"#,
            )
            .unwrap();
        }
        let (received, request) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "registry fetch never started");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("registry listener: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            received.send(()).unwrap();
            // Hold Git's response headers until the daemon has shut down.
            let _ = wait.recv();
        });
        let (mut command, address) = command(root.path(), &app);
        command.env("AMARCODE_ACP_REGISTRY_SOURCE", &source);
        let mut daemon = Daemon {
            child: command.spawn().unwrap(),
            app: app.clone(),
        };
        request
            .recv_timeout(Duration::from_secs(5))
            .expect("registry HTTP request");
        ready(&mut daemon, &address);
        let mut stream = TcpStream::connect(&address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        writeln!(stream, "{{\"method\":\"list_agents\"}}").unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        assert!(reply.contains("amarcode-acp"), "{reply}");
        if cached {
            assert!(reply.contains("cached-agent"), "{reply}");
        }
        stop(&mut daemon);
        release.send(()).unwrap();
        server.join().unwrap();
    }
}

#[test]
#[cfg(not(windows))]
fn background_registry_refresh_publishes_new_agents() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::create_dir_all(source.join("fresh-agent")).unwrap();
    for name in ["agent.schema.json", "registry.schema.json"] {
        std::fs::write(source.join(name), "{}").unwrap();
    }
    std::fs::write(
        source.join("fresh-agent/agent.json"),
        r#"{
        "id":"fresh-agent", "name":"Fresh Agent", "version":"1.0.0",
        "distribution":{"npx":{"package":"fresh-agent@1.0.0"}}
    }"#,
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet", "--initial-branch=main"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    ] {
        assert!(Command::new("git")
            .current_dir(&source)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    let app = root.path().join("data");
    let (mut command, address) = command(root.path(), &app);
    command.env("AMARCODE_ACP_REGISTRY_SOURCE", &source);
    let mut daemon = Daemon {
        child: command.spawn().unwrap(),
        app,
    };
    ready(&mut daemon, &address);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut stream = TcpStream::connect(&address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        writeln!(stream, "{{\"method\":\"list_agents\"}}").unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        if reply.contains("fresh-agent") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "refreshed catalog not published: {reply}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    stop(&mut daemon);
}
