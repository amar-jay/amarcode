//! Per-user, login-persistent daemon registration.
//!
//! Production daemon releases are owned by the platform service manager:
//! systemd's user manager on Linux, a LaunchAgent on macOS, and a per-user
//! logon task on Windows. Development overrides deliberately bypass this
//! module so a binary in `target/debug` is never installed persistently.

use std::{
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Output},
};

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
const SERVICE_LABEL: &str = "com.amarcode.daemon";

pub fn install_and_start(executable: &Path, agent_path: Option<&OsStr>) -> Result<(), String> {
    let executable = executable
        .canonicalize()
        .map_err(|error| format!("failed to resolve daemon executable: {error}"))?;

    #[cfg(target_os = "linux")]
    {
        linux::install_and_start(&executable, agent_path)
    }

    #[cfg(target_os = "macos")]
    {
        macos::install_and_start(&executable, agent_path)
    }

    #[cfg(target_os = "windows")]
    {
        let _ = agent_path;
        windows::install_and_start(&executable)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = agent_path;
        Err("per-user daemon services are not supported on this platform".into())
    }
}

#[cfg(unix)]
fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "cannot install the daemon service because HOME is not set".into())
}

#[cfg(unix)]
fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("service path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let temporary = path.with_extension(format!("{}.part", std::process::id()));
    fs::write(&temporary, contents)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("failed to replace {}: {error}", path.display()))?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| format!("failed to install {}: {error}", path.display()))
}

fn checked(command: &mut Command, operation: &str) -> Result<Output, String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to {operation}: {error}"))?;
    if output.status.success() {
        return Ok(output);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(if detail.is_empty() {
        format!("failed to {operation}: {}", output.status)
    } else {
        format!("failed to {operation}: {detail}")
    })
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    const UNIT_NAME: &str = "amarcode-daemon.service";

    pub fn install_and_start(executable: &Path, agent_path: Option<&OsStr>) -> Result<(), String> {
        let config_root = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or(home_dir()?.join(".config"));
        let unit_path = config_root.join("systemd/user").join(UNIT_NAME);
        let executable = systemd_quote(executable.as_os_str())?;
        let path_environment = agent_path
            .map(systemd_quote)
            .transpose()?
            .map(|path| format!("Environment=\"PATH={path}\"\n"))
            .unwrap_or_default();
        let unit = format!(
            "[Unit]\nDescription=Amarcode background service\n\n\
             [Service]\nType=simple\nExecStart=\"{executable}\"\n{path_environment}\
             Restart=on-failure\nRestartSec=2\nTimeoutStopSec=15\n\n\
             [Install]\nWantedBy=default.target\n"
        );
        write_atomic(&unit_path, unit.as_bytes())?;

        checked(
            Command::new("systemctl").args(["--user", "daemon-reload"]),
            "reload the systemd user manager",
        )?;
        checked(
            Command::new("systemctl").args(["--user", "enable", UNIT_NAME]),
            "enable the Amarcode user service",
        )?;
        checked(
            Command::new("systemctl").args(["--user", "restart", UNIT_NAME]),
            "start the Amarcode user service",
        )?;
        Ok(())
    }

    fn systemd_quote(value: &OsStr) -> Result<String, String> {
        let value = value
            .to_str()
            .ok_or("daemon service paths must contain valid UTF-8")?;
        Ok(value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('\n', "\\n")
            .replace('\r', "\\r"))
    }

    #[cfg(test)]
    mod tests {
        use super::systemd_quote;
        use std::ffi::OsStr;

        #[test]
        fn escapes_systemd_specifiers_and_quotes() {
            assert_eq!(
                systemd_quote(OsStr::new("/tmp/a %i/\"daemon\"")).unwrap(),
                "/tmp/a %%i/\\\"daemon\\\""
            );
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    pub fn install_and_start(executable: &Path, agent_path: Option<&OsStr>) -> Result<(), String> {
        let launch_agents = home_dir()?.join("Library/LaunchAgents");
        let plist_path = launch_agents.join(format!("{SERVICE_LABEL}.plist"));
        let executable = xml_escape(
            executable
                .to_str()
                .ok_or("daemon service paths must contain valid UTF-8")?,
        );
        let path_environment = agent_path
            .and_then(OsStr::to_str)
            .map(xml_escape)
            .map(|path| {
                format!(
                    "<key>EnvironmentVariables</key><dict><key>PATH</key><string>{path}</string></dict>"
                )
            })
            .unwrap_or_default();
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\
             <key>Label</key><string>{SERVICE_LABEL}</string>\
             <key>ProgramArguments</key><array><string>{executable}</string></array>\
             <key>RunAtLoad</key><true/>\
             <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\
             <key>ProcessType</key><string>Background</string>\
             {path_environment}\
             </dict></plist>\n"
        );
        write_atomic(&plist_path, plist.as_bytes())?;

        let uid_output = checked(Command::new("id").arg("-u"), "resolve the current user id")?;
        let uid = String::from_utf8(uid_output.stdout)
            .map_err(|_| "current user id was not valid UTF-8".to_string())?;
        let domain = format!("gui/{}", uid.trim());
        let service = format!("{domain}/{SERVICE_LABEL}");

        // Unloading an absent service is expected on first install.
        let _ = Command::new("launchctl")
            .args(["bootout", &service])
            .output();
        checked(
            Command::new("launchctl")
                .arg("bootstrap")
                .arg(&domain)
                .arg(&plist_path),
            "register the Amarcode LaunchAgent",
        )?;
        checked(
            Command::new("launchctl").args(["kickstart", "-k", &service]),
            "start the Amarcode LaunchAgent",
        )?;
        Ok(())
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;

    const TASK_NAME: &str = "Amarcode Daemon";

    pub fn install_and_start(executable: &Path) -> Result<(), String> {
        let user_output = checked(
            &mut Command::new("whoami.exe"),
            "resolve the current Windows user",
        )?;
        let user = String::from_utf8(user_output.stdout)
            .map_err(|_| "current Windows user was not valid UTF-8".to_string())?;
        let task = task_xml(executable, user.trim())?;
        let task_file =
            std::env::temp_dir().join(format!("amarcode-daemon-task-{}.xml", std::process::id()));
        fs::write(&task_file, task)
            .map_err(|error| format!("failed to write {}: {error}", task_file.display()))?;

        // Stop an old registration before replacing its executable path.
        let _ = Command::new("schtasks.exe")
            .args(["/End", "/TN", TASK_NAME])
            .output();
        let registration = checked(
            Command::new("schtasks.exe")
                .args(["/Create", "/TN", TASK_NAME, "/XML"])
                .arg(&task_file)
                .arg("/F"),
            "register the Amarcode logon task",
        );
        let _ = fs::remove_file(&task_file);
        registration?;
        checked(
            Command::new("schtasks.exe").args(["/Run", "/TN", TASK_NAME]),
            "start the Amarcode logon task",
        )?;
        Ok(())
    }

    fn task_xml(executable: &Path, user: &str) -> Result<String, String> {
        let working_directory = executable
            .parent()
            .and_then(Path::to_str)
            .ok_or("daemon service directory must contain valid UTF-8")?;
        let executable = executable
            .to_str()
            .ok_or("daemon service paths must contain valid UTF-8")?;
        let executable = xml_escape(executable);
        let working_directory = xml_escape(working_directory);
        let user = xml_escape(user);
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\
             <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>\
             <Principals><Principal id=\"Author\"><UserId>{user}</UserId>\
             <LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel>\
             </Principal></Principals>\
             <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\
             <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\
             <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\
             <AllowHardTerminate>true</AllowHardTerminate>\
             <StartWhenAvailable>true</StartWhenAvailable>\
             <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\
             <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\
             <RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure>\
             <Enabled>true</Enabled></Settings>\
             <Actions Context=\"Author\"><Exec><Command>{executable}</Command>\
             <WorkingDirectory>{working_directory}</WorkingDirectory></Exec></Actions>\
             </Task>"
        ))
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}
