//! Native per-user service lifecycle management.
//!
//! This module is intentionally owned by the daemon binary. Desktop and CLI
//! clients may invoke the daemon's short-lived lifecycle commands, but they do
//! not write native service definitions themselves.

use std::{
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Output},
};

#[cfg(unix)]
use std::path::PathBuf;

use serde::Serialize;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    NotInstalled,
    Stopped,
    Running,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub installed: bool,
    pub state: ServiceState,
    pub definition_path: Option<String>,
}

pub trait ServiceController {
    fn install(&self, executable: &Path, agent_path: Option<&OsStr>) -> Result<()>;
    fn uninstall(&self) -> Result<()>;
    fn start(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn restart(&self) -> Result<()>;
    fn status(&self) -> Result<ServiceStatus>;
}

pub struct NativeServiceController;

impl ServiceController for NativeServiceController {
    fn install(&self, executable: &Path, agent_path: Option<&OsStr>) -> Result<()> {
        let executable = executable.canonicalize().map_err(|error| {
            Error::msg(format!(
                "failed to resolve daemon executable {}: {error}",
                executable.display()
            ))
        })?;
        platform::install(&executable, agent_path)
    }

    fn uninstall(&self) -> Result<()> {
        platform::uninstall()
    }

    fn start(&self) -> Result<()> {
        platform::start()
    }

    fn stop(&self) -> Result<()> {
        platform::stop()
    }

    fn restart(&self) -> Result<()> {
        platform::restart()
    }

    fn status(&self) -> Result<ServiceStatus> {
        platform::status()
    }
}

fn checked(command: &mut Command, operation: &str) -> Result<Output> {
    let output = command
        .output()
        .map_err(|error| Error::msg(format!("failed to {operation}: {error}")))?;
    if output.status.success() {
        return Ok(output);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(Error::msg(if detail.is_empty() {
        format!("failed to {operation}: {}", output.status)
    } else {
        format!("failed to {operation}: {detail}")
    }))
}

#[cfg(unix)]
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::msg(format!("service path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent)
        .map_err(|error| Error::msg(format!("failed to create {}: {error}", parent.display())))?;
    let temporary = path.with_extension(format!("{}.part", std::process::id()));
    fs::write(&temporary, contents)
        .map_err(|error| Error::msg(format!("failed to write {}: {error}", temporary.display())))?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            Error::msg(format!("failed to replace {}: {error}", path.display()))
        })?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| Error::msg(format!("failed to install {}: {error}", path.display())))
}

#[cfg(unix)]
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::msg("cannot manage the daemon service because HOME is not set"))
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    const UNIT_NAME: &str = "amarcode-daemon.service";

    fn unit_path() -> Result<PathBuf> {
        let config_root = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or(home_dir()?.join(".config"));
        Ok(config_root.join("systemd/user").join(UNIT_NAME))
    }

    pub fn install(executable: &Path, agent_path: Option<&OsStr>) -> Result<()> {
        let path = unit_path()?;
        let unit = systemd_unit(executable, agent_path)?;
        write_atomic(&path, unit.as_bytes())?;
        checked(
            Command::new("systemctl").args(["--user", "daemon-reload"]),
            "reload the systemd user manager",
        )?;
        checked(
            Command::new("systemctl").args(["--user", "enable", UNIT_NAME]),
            "enable the Amarcode user service",
        )?;
        Ok(())
    }

    fn systemd_unit(executable: &Path, agent_path: Option<&OsStr>) -> Result<String> {
        let executable = systemd_quote(executable.as_os_str())?;
        let path_environment = agent_path
            .map(systemd_quote)
            .transpose()?
            .map(|value| format!("Environment=\"PATH={value}\"\n"))
            .unwrap_or_default();
        Ok(format!(
            "[Unit]\nDescription=Amarcode background service\nAfter=network.target\n\n\
             [Service]\nType=simple\nExecStart=\"{executable}\" run\n{path_environment}\
             Restart=on-failure\nRestartSec=2\nTimeoutStopSec=15\n\n\
             [Install]\nWantedBy=default.target\n"
        ))
    }

    pub fn uninstall() -> Result<()> {
        let path = unit_path()?;
        if !path.exists() {
            return Ok(());
        }
        let _ = Command::new("systemctl")
            .args(["--user", "stop", UNIT_NAME])
            .output();
        let _ = Command::new("systemctl")
            .args(["--user", "disable", UNIT_NAME])
            .output();
        fs::remove_file(&path)
            .map_err(|error| Error::msg(format!("failed to remove {}: {error}", path.display())))?;
        checked(
            Command::new("systemctl").args(["--user", "daemon-reload"]),
            "reload the systemd user manager",
        )?;
        Ok(())
    }

    pub fn start() -> Result<()> {
        ensure_installed()?;
        checked(
            Command::new("systemctl").args(["--user", "start", UNIT_NAME]),
            "start the Amarcode user service",
        )?;
        Ok(())
    }

    pub fn stop() -> Result<()> {
        if !unit_path()?.exists() {
            return Ok(());
        }
        checked(
            Command::new("systemctl").args(["--user", "stop", UNIT_NAME]),
            "stop the Amarcode user service",
        )?;
        Ok(())
    }

    pub fn restart() -> Result<()> {
        ensure_installed()?;
        checked(
            Command::new("systemctl").args(["--user", "restart", UNIT_NAME]),
            "restart the Amarcode user service",
        )?;
        Ok(())
    }

    pub fn status() -> Result<ServiceStatus> {
        let path = unit_path()?;
        if !path.exists() {
            return Ok(ServiceStatus {
                installed: false,
                state: ServiceState::NotInstalled,
                definition_path: Some(path.display().to_string()),
            });
        }
        let active = Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", UNIT_NAME])
            .status()
            .map_err(|error| Error::msg(format!("failed to query Amarcode service: {error}")))?;
        Ok(ServiceStatus {
            installed: true,
            state: if active.success() {
                ServiceState::Running
            } else {
                ServiceState::Stopped
            },
            definition_path: Some(path.display().to_string()),
        })
    }

    fn ensure_installed() -> Result<()> {
        if unit_path()?.exists() {
            Ok(())
        } else {
            Err(Error::msg(
                "Amarcode background service is not installed; run `amarcode-daemon install` first",
            ))
        }
    }

    fn systemd_quote(value: &OsStr) -> Result<String> {
        let value = value
            .to_str()
            .ok_or_else(|| Error::msg("daemon service paths must contain valid UTF-8"))?;
        Ok(value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('\n', "\\n")
            .replace('\r', "\\r"))
    }

    #[cfg(test)]
    mod tests {
        use super::{systemd_quote, systemd_unit};
        use std::ffi::OsStr;

        #[test]
        fn escapes_systemd_specifiers_and_quotes() {
            assert_eq!(
                systemd_quote(OsStr::new("/tmp/a %i/\"daemon\"")).unwrap(),
                "/tmp/a %%i/\\\"daemon\\\""
            );
        }

        #[test]
        fn service_definition_runs_the_explicit_foreground_command() {
            let unit = systemd_unit(
                std::path::Path::new("/opt/amarcode/amarcode-daemon"),
                Some(OsStr::new("/usr/bin")),
            )
            .unwrap();
            assert!(unit.contains("ExecStart=\"/opt/amarcode/amarcode-daemon\" run"));
            assert!(unit.contains("Environment=\"PATH=/usr/bin\""));
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    const SERVICE_LABEL: &str = "com.amarcode.daemon";

    fn plist_path() -> Result<PathBuf> {
        Ok(home_dir()?
            .join("Library/LaunchAgents")
            .join(format!("{SERVICE_LABEL}.plist")))
    }

    fn domain_and_service() -> Result<(String, String)> {
        let uid_output = checked(Command::new("id").arg("-u"), "resolve the current user id")?;
        let uid = String::from_utf8(uid_output.stdout)
            .map_err(|_| Error::msg("current user id was not valid UTF-8"))?;
        let domain = format!("gui/{}", uid.trim());
        let service = format!("{domain}/{SERVICE_LABEL}");
        Ok((domain, service))
    }

    pub fn install(executable: &Path, agent_path: Option<&OsStr>) -> Result<()> {
        let path = plist_path()?;
        let executable = xml_escape(
            executable
                .to_str()
                .ok_or_else(|| Error::msg("daemon service paths must contain valid UTF-8"))?,
        );
        let path_environment = agent_path
            .and_then(OsStr::to_str)
            .map(xml_escape)
            .map(|value| {
                format!(
                    "<key>EnvironmentVariables</key><dict><key>PATH</key><string>{value}</string></dict>"
                )
            })
            .unwrap_or_default();
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\
             <key>Label</key><string>{SERVICE_LABEL}</string>\
             <key>ProgramArguments</key><array><string>{executable}</string><string>run</string></array>\
             <key>RunAtLoad</key><true/>\
             <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\
             <key>ProcessType</key><string>Background</string>\
             {path_environment}\
             </dict></plist>\n"
        );
        write_atomic(&path, plist.as_bytes())
    }

    pub fn uninstall() -> Result<()> {
        let path = plist_path()?;
        let _ = stop();
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                Error::msg(format!("failed to remove {}: {error}", path.display()))
            })?;
        }
        Ok(())
    }

    pub fn start() -> Result<()> {
        let path = plist_path()?;
        if !path.exists() {
            return Err(Error::msg(
                "Amarcode background service is not installed; run `amarcode-daemon install` first",
            ));
        }
        let (domain, service) = domain_and_service()?;
        let loaded = Command::new("launchctl")
            .args(["print", &service])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if !loaded {
            checked(
                Command::new("launchctl")
                    .arg("bootstrap")
                    .arg(&domain)
                    .arg(&path),
                "register the Amarcode LaunchAgent",
            )?;
        }
        checked(
            Command::new("launchctl").args(["kickstart", "-k", &service]),
            "start the Amarcode LaunchAgent",
        )?;
        Ok(())
    }

    pub fn stop() -> Result<()> {
        if !plist_path()?.exists() {
            return Ok(());
        }
        let (_, service) = domain_and_service()?;
        let output = Command::new("launchctl")
            .args(["bootout", &service])
            .output()
            .map_err(|error| Error::msg(format!("failed to stop Amarcode LaunchAgent: {error}")))?;
        if output.status.success()
            || String::from_utf8_lossy(&output.stderr).contains("No such process")
        {
            Ok(())
        } else {
            Err(Error::msg(format!(
                "failed to stop Amarcode LaunchAgent: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    pub fn restart() -> Result<()> {
        stop()?;
        start()
    }

    pub fn status() -> Result<ServiceStatus> {
        let path = plist_path()?;
        if !path.exists() {
            return Ok(ServiceStatus {
                installed: false,
                state: ServiceState::NotInstalled,
                definition_path: Some(path.display().to_string()),
            });
        }
        let (_, service) = domain_and_service()?;
        let running = Command::new("launchctl")
            .args(["print", &service])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        Ok(ServiceStatus {
            installed: true,
            state: if running {
                ServiceState::Running
            } else {
                ServiceState::Stopped
            },
            definition_path: Some(path.display().to_string()),
        })
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
mod platform {
    use super::*;

    const TASK_NAME: &str = "Amarcode Daemon";

    pub fn install(executable: &Path, _agent_path: Option<&OsStr>) -> Result<()> {
        let user_output = checked(
            &mut Command::new("whoami.exe"),
            "resolve the current Windows user",
        )?;
        let user = String::from_utf8(user_output.stdout)
            .map_err(|_| Error::msg("current Windows user was not valid UTF-8"))?;
        let task = task_xml(executable, user.trim())?;
        let task_file =
            std::env::temp_dir().join(format!("amarcode-daemon-task-{}.xml", std::process::id()));
        fs::write(&task_file, task).map_err(|error| {
            Error::msg(format!("failed to write {}: {error}", task_file.display()))
        })?;

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
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        if !status()?.installed {
            return Ok(());
        }
        let _ = Command::new("schtasks.exe")
            .args(["/End", "/TN", TASK_NAME])
            .output();
        checked(
            Command::new("schtasks.exe").args(["/Delete", "/TN", TASK_NAME, "/F"]),
            "remove the Amarcode logon task",
        )?;
        Ok(())
    }

    pub fn start() -> Result<()> {
        checked(
            Command::new("schtasks.exe").args(["/Run", "/TN", TASK_NAME]),
            "start the Amarcode logon task",
        )?;
        Ok(())
    }

    pub fn stop() -> Result<()> {
        if !status()?.installed {
            return Ok(());
        }
        checked(
            Command::new("schtasks.exe").args(["/End", "/TN", TASK_NAME]),
            "stop the Amarcode logon task",
        )?;
        Ok(())
    }

    pub fn restart() -> Result<()> {
        stop()?;
        start()
    }

    pub fn status() -> Result<ServiceStatus> {
        let output = Command::new("schtasks.exe")
            .args(["/Query", "/TN", TASK_NAME, "/FO", "LIST", "/V"])
            .output()
            .map_err(|error| Error::msg(format!("failed to query Amarcode logon task: {error}")))?;
        if !output.status.success() {
            return Ok(ServiceStatus {
                installed: false,
                state: ServiceState::NotInstalled,
                definition_path: None,
            });
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(ServiceStatus {
            installed: true,
            state: if text.lines().any(|line| line.contains("Running")) {
                ServiceState::Running
            } else {
                ServiceState::Stopped
            },
            definition_path: None,
        })
    }

    fn task_xml(executable: &Path, user: &str) -> Result<String> {
        let working_directory = executable
            .parent()
            .and_then(Path::to_str)
            .ok_or_else(|| Error::msg("daemon service directory must contain valid UTF-8"))?;
        let executable = executable
            .to_str()
            .ok_or_else(|| Error::msg("daemon service paths must contain valid UTF-8"))?;
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\
             <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{}</UserId></LogonTrigger></Triggers>\
             <Principals><Principal id=\"Author\"><UserId>{}</UserId>\
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
             <Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>run</Arguments>\
             <WorkingDirectory>{}</WorkingDirectory></Exec></Actions>\
             </Task>",
            xml_escape(user),
            xml_escape(user),
            xml_escape(executable),
            xml_escape(working_directory),
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

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::*;

    fn unsupported<T>() -> Result<T> {
        Err(Error::msg(
            "per-user daemon services are not supported on this platform",
        ))
    }

    pub fn install(_executable: &Path, _agent_path: Option<&OsStr>) -> Result<()> {
        unsupported()
    }
    pub fn uninstall() -> Result<()> {
        unsupported()
    }
    pub fn start() -> Result<()> {
        unsupported()
    }
    pub fn stop() -> Result<()> {
        unsupported()
    }
    pub fn restart() -> Result<()> {
        unsupported()
    }
    pub fn status() -> Result<ServiceStatus> {
        unsupported()
    }
}
