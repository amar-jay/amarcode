//! Keep Linux agent/tool process trees out of the daemon's service cgroup.
use std::{
    ffi::OsStr,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// Owns the scope as well as the directly spawned process. Killing only the
/// systemd-run wrapper would leave grandchildren (including detached tools).
pub(crate) struct Workload {
    scope: Option<String>,
    bind_to_daemon: bool,
    terminated: AtomicBool,
}

impl Workload {
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        if managed_service() {
            let mut workload = Self::isolated();
            workload.bind_to_daemon = in_daemon_service();
            return workload;
        }
        Self {
            scope: None,
            bind_to_daemon: false,
            terminated: AtomicBool::new(false),
        }
    }

    #[cfg(target_os = "linux")]
    fn isolated() -> Self {
        Self {
            scope: Some(format!(
                "amarcode-workload-{}.scope",
                uuid::Uuid::new_v4().simple()
            )),
            bind_to_daemon: false,
            terminated: AtomicBool::new(false),
        }
    }

    fn argv(&self, program: &OsStr) -> Vec<std::ffi::OsString> {
        let Some(scope) = &self.scope else {
            return vec![program.to_owned()];
        };
        let mut argv = vec![
            "systemd-run".into(),
            "--user".into(),
            "--scope".into(),
            "--quiet".into(),
            "--collect".into(),
            "--description=Amarcode workload".into(),
            "--expand-environment=no".into(),
            format!("--unit={scope}").into(),
            "--slice=amarcode-workloads.slice".into(),
            "--property=MemoryAccounting=yes".into(),
            "--property=OOMPolicy=kill".into(),
            format!(
                "--property=MemoryHigh={}",
                std::env::var("AMARCODE_WORKLOAD_MEMORY_HIGH").unwrap_or_else(|_| "25%".into())
            )
            .into(),
            format!(
                "--property=MemoryMax={}",
                std::env::var("AMARCODE_WORKLOAD_MEMORY_MAX").unwrap_or_else(|_| "50%".into())
            )
            .into(),
            format!(
                "--property=MemorySwapMax={}",
                std::env::var("AMARCODE_WORKLOAD_MEMORY_SWAP_MAX").unwrap_or_else(|_| "10%".into())
            )
            .into(),
            "--property=TimeoutStopSec=5s".into(),
        ];
        if self.bind_to_daemon {
            argv.extend([
                "--property=BindsTo=amarcode-daemon.service".into(),
                "--property=After=amarcode-daemon.service".into(),
            ]);
        }
        argv.extend(["--".into(), program.to_owned()]);
        argv
    }

    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let argv = self.argv(program.as_ref());
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        command
    }

    pub fn pty_command(&self, program: impl AsRef<OsStr>) -> portable_pty::CommandBuilder {
        let argv = self.argv(program.as_ref());
        let mut command = portable_pty::CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        command
    }

    pub fn terminate(&self) {
        let Some(scope) = &self.scope else {
            return;
        };
        if self.terminated.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(mut child) = Command::new("systemctl")
            .args(["--user", "kill", "--signal=KILL", scope])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        // Never let a missing/unresponsive manager hang Stop or daemon exit.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                _ if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::warn!(%scope, "timed out terminating workload scope");
                    break;
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

impl Drop for Workload {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(target_os = "linux")]
fn managed_service() -> bool {
    // Explicit policy is written into installed services. Also recognize older
    // registrations, so a binary upgrade does not silently lose isolation.
    match std::env::var("AMARCODE_WORKLOAD_ISOLATION").as_deref() {
        Ok("systemd") => true,
        Ok("none") => false,
        _ => in_daemon_service(),
    }
}

#[cfg(target_os = "linux")]
fn in_daemon_service() -> bool {
    std::fs::read_to_string("/proc/self/cgroup").is_ok_and(|text| {
        text.lines().any(|line| {
            line.splitn(3, ':').nth(2).is_some_and(|path| {
                path.split('/')
                    .any(|part| part == "amarcode-daemon.service")
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_commands_preserve_literal_arguments() {
        let workload = Workload {
            scope: None,
            bind_to_daemon: false,
            terminated: AtomicBool::new(false),
        };
        assert_eq!(
            workload.argv(OsStr::new("/tmp/agent $name")),
            vec![std::ffi::OsString::from("/tmp/agent $name")]
        );
    }

    #[cfg(target_os = "linux")]
    fn test_scope() -> Workload {
        let mut workload = Workload::isolated();
        // Tests must not bind to, start, stop, or reconfigure the installed daemon.
        workload.bind_to_daemon = false;
        workload
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a systemd user manager and cgroup v2"]
    fn systemd_scope_preserves_pipes_environment_cwd_and_exit_status() {
        use std::io::Write;
        let workload = test_scope();
        let directory = tempfile::tempdir().unwrap();
        let mut child = workload.command("/bin/sh")
            .args(["-c", "printf '%s\\n' \"$1\" \"$PWD\" \"$AMARCODE_SCOPE_TEST\"; cat /proc/self/cgroup; cat; exit 7", "sh", "$literal ${unchanged}"])
            .env("AMARCODE_SCOPE_TEST", "environment preserved")
            .current_dir(directory.path())
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"ACP stdin preserved\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(7),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.starts_with("$literal ${unchanged}\n"), "{text}");
        assert!(text.contains(directory.path().to_str().unwrap()));
        assert!(text.contains("environment preserved\n"));
        assert!(text.contains("/amarcode-workloads.slice/amarcode-workload-"));
        assert!(!text.contains("/amarcode-daemon.service/"));
        assert!(text.ends_with("ACP stdin preserved\n"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a systemd user manager and cgroup v2"]
    fn memory_limit_kills_only_the_isolated_workload() {
        let workload = test_scope();
        let mut argv = workload.argv(OsStr::new("/usr/bin/python3"));
        for arg in &mut argv {
            if arg.to_string_lossy().starts_with("--property=MemoryHigh=") {
                // Exercise the hard limit without the soft-limit reclaim delay.
                *arg = "--property=MemoryHigh=infinity".into();
            }
            if arg.to_string_lossy().starts_with("--property=MemoryMax=") {
                *arg = "--property=MemoryMax=32M".into();
            }
            if arg
                .to_string_lossy()
                .starts_with("--property=MemorySwapMax=")
            {
                *arg = "--property=MemorySwapMax=0".into();
            }
        }
        let parent_group = std::fs::read_to_string("/proc/self/cgroup").unwrap();
        let output = Command::new(&argv[0]).args(&argv[1..]).args(["-c", "import pathlib,subprocess; p=pathlib.Path('/proc/self/cgroup').read_text().strip().split(':',2)[2]; print('limit='+pathlib.Path('/sys/fs/cgroup'+p+'/memory.max').read_text().strip(),flush=True); print('descendant='+str(subprocess.Popen(['/bin/sleep','60'],start_new_session=True).pid),flush=True); a=bytearray(128*1024*1024); a[::4096]=b'x'*(len(a)//4096)"])
            .output().unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("limit=33554432"),
            "{output:?}"
        );
        assert!(
            !output.status.success(),
            "workload exceeded its memory limit"
        );
        assert_eq!(
            std::fs::read_to_string("/proc/self/cgroup").unwrap(),
            parent_group
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let pid = text
            .lines()
            .find_map(|line| line.strip_prefix("descendant="))
            .unwrap();
        assert_process_exited(pid);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a systemd user manager and cgroup v2"]
    fn scope_cleanup_kills_detached_descendants() {
        let workload = test_scope();
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let output = workload
            .command("/bin/sh")
            .args([
                "-c",
                "setsid sleep 60 </dev/null >/dev/null 2>&1 & echo $! > \"$1\"",
                "sh",
            ])
            .arg(&pid_file)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let pid = std::fs::read_to_string(pid_file).unwrap();
        workload.terminate();
        assert_process_exited(pid.trim());
    }

    #[cfg(target_os = "linux")]
    fn assert_process_exited(pid: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            if status.is_err()
                || status
                    .unwrap()
                    .split(')')
                    .nth(1)
                    .unwrap()
                    .trim_start()
                    .starts_with('Z')
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "detached child survived scope cleanup"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a systemd user manager and cgroup v2"]
    fn isolated_terminal_preserves_pty_size_and_exit_status() {
        use portable_pty::{native_pty_system, PtySize};
        use std::io::Read;
        let workload = test_scope();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = workload.pty_command("/bin/sh");
        command.args(["-c", "stty size; cat /proc/self/cgroup; exit 11"]);
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let status = child.wait().unwrap();
        let mut bytes = Vec::new();
        // Linux PTYs may return EIO at EOF after the slave exits.
        let _ = reader.read_to_end(&mut bytes);
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(status.exit_code(), 11, "{text}");
        assert!(text.contains("24 80"), "{text}");
        assert!(
            text.contains("/amarcode-workloads.slice/amarcode-workload-"),
            "{text}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a systemd user manager and cgroup v2"]
    fn isolated_interactive_terminal_preserves_ctrl_c() {
        use portable_pty::{native_pty_system, PtySize};
        use std::io::{Read, Write};
        let workload = test_scope();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = workload.pty_command("/bin/sh");
        command.arg("-i");
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut writer = pair.master.take_writer().unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 || send.send(bytes[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        writer
            .write_all(b"echo WORKLOAD_STARTED; sleep 60\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut output = String::new();
        while !output
            .replace('\r', "")
            .lines()
            .any(|line| line.trim_end().ends_with("WORKLOAD_STARTED"))
        {
            let chunk = receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| panic!("waiting for shell output: {error}; {output:?}"));
            output.push_str(&String::from_utf8_lossy(&chunk));
        }
        // Let the shell hand the foreground terminal to sleep before sending
        // SIGINT, then let the terminal flush interrupted input before typing.
        std::thread::sleep(Duration::from_millis(100));
        writer.write_all(b"\x03").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        writer
            .write_all(b"echo WORKLOAD_FINISHED; exit 0\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.exit_code(), 0);
                break;
            }
            if Instant::now() >= deadline {
                workload.terminate();
                let _ = child.wait();
                for chunk in receive.try_iter() {
                    output.push_str(&String::from_utf8_lossy(&chunk));
                }
                panic!("Ctrl-C did not interrupt the isolated command: {output:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
