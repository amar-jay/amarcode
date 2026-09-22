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

#[cfg(any(target_os = "windows", test))]
fn utf16le_with_bom(value: &str) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(2 + value.len() * 2);
    encoded.extend_from_slice(&[0xff, 0xfe]);
    for unit in value.encode_utf16() {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    encoded
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
        checked(
            Command::new("systemctl").args(["--user", "stop", UNIT_NAME]),
            "stop the Amarcode user service before uninstalling it",
        )?;
        checked(
            Command::new("systemctl").args(["--user", "disable", UNIT_NAME]),
            "disable the Amarcode user service",
        )?;
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
    use windows_sys::Win32::Security::Authentication::Identity::{
        GetUserNameExW, NameSamCompatible,
    };

    const TASK_NAME: &str = "Amarcode Daemon";

    fn command(program: &str) -> Command {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(program);
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    }

    pub fn install(executable: &Path, agent_path: Option<&OsStr>) -> Result<()> {
        let user = current_user()?;
        let task = task_xml(executable, &user)?;
        let task_file =
            std::env::temp_dir().join(format!("amarcode-daemon-task-{}.xml", std::process::id()));
        fs::write(&task_file, utf16le_with_bom(&task)).map_err(|error| {
            Error::msg(format!("failed to write {}: {error}", task_file.display()))
        })?;

        crate::windows::save_service_path(agent_path)?;
        let _ = command("schtasks.exe")
            .args(["/End", "/TN", TASK_NAME])
            .output();
        let registration = checked(
            command("schtasks.exe")
                .args(["/Create", "/TN", TASK_NAME, "/XML"])
                .arg(&task_file)
                .arg("/F"),
            "register the Amarcode logon task",
        );
        let _ = fs::remove_file(&task_file);
        registration?;
        Ok(())
    }

    fn current_user() -> Result<String> {
        let mut buffer = vec![0_u16; 256];
        loop {
            let mut length = buffer.len() as u32;
            // SAFETY: `buffer` is writable for `length` UTF-16 code units, and
            // `length` remains valid for the duration of the call.
            let succeeded =
                unsafe { GetUserNameExW(NameSamCompatible, buffer.as_mut_ptr(), &mut length) };
            if succeeded {
                return String::from_utf16(&buffer[..length as usize]).map_err(|error| {
                    Error::msg(format!(
                        "current Windows user name was not valid UTF-16: {error}"
                    ))
                });
            }
            if length as usize > buffer.len() {
                buffer.resize(length as usize, 0);
                continue;
            }
            return Err(Error::msg(format!(
                "failed to resolve the current Windows user: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    pub fn uninstall() -> Result<()> {
        if !status()?.installed {
            return if daemon_process_running()? {
                Err(Error::msg(
                    "amarcode-daemon.exe is still running without a registered scheduled task",
                ))
            } else {
                crate::windows::clear_service_path()
            };
        }
        let _ = command("schtasks.exe")
            .args(["/End", "/TN", TASK_NAME])
            .output();
        for _ in 0..20 {
            if !daemon_process_running()? {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if daemon_process_running()? {
            return Err(Error::msg(
                "failed to stop amarcode-daemon.exe before removing its scheduled task",
            ));
        }
        checked(
            command("schtasks.exe").args(["/Delete", "/TN", TASK_NAME, "/F"]),
            "remove the Amarcode logon task",
        )?;
        crate::windows::clear_service_path()
    }

    fn daemon_process_running() -> Result<bool> {
        let output = command("tasklist.exe")
            .args([
                "/FI",
                "IMAGENAME eq amarcode-daemon.exe",
                "/FO",
                "CSV",
                "/NH",
            ])
            .output()
            .map_err(|error| Error::msg(format!("failed to verify daemon exit: {error}")))?;
        if !output.status.success() {
            return Err(Error::msg(format!(
                "failed to verify daemon exit: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.starts_with("\"amarcode-daemon.exe\",")))
    }

    pub fn start() -> Result<()> {
        if status()?.state == ServiceState::Running {
            return Ok(());
        }
        checked(
            command("schtasks.exe").args(["/Run", "/TN", TASK_NAME]),
            "start the Amarcode logon task",
        )?;
        Ok(())
    }

    pub fn stop() -> Result<()> {
        let current = status()?;
        if !current.installed || current.state == ServiceState::Stopped {
            return Ok(());
        }
        checked(
            command("schtasks.exe").args(["/End", "/TN", TASK_NAME]),
            "stop the Amarcode logon task",
        )?;
        // /End acknowledges a stop request before the task necessarily exits.
        // Wait before restart's idempotent start checks whether it is running.
        for _ in 0..20 {
            let current = status()?;
            if !current.installed || current.state == ServiceState::Stopped {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err(Error::msg("Amarcode logon task did not stop"))
    }

    pub fn restart() -> Result<()> {
        stop()?;
        start()
    }

    pub fn status() -> Result<ServiceStatus> {
        // schtasks /Query is localized. Use the scheduler's numeric COM state
        // and distinguish a missing task from access/COM failures.
        let output = checked(
            command("powershell.exe").args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                include_str!("windows_task_status.ps1"),
            ]),
            "query Amarcode logon task state",
        )?;
        task_status_from_output(&String::from_utf8_lossy(&output.stdout))
    }

    fn task_status_from_output(output: &str) -> Result<ServiceStatus> {
        let state = match output.trim() {
            "-1" => ServiceState::NotInstalled,
            "0" | "2" => ServiceState::Unknown,
            "1" | "3" => ServiceState::Stopped,
            "4" => ServiceState::Running,
            value => {
                return Err(Error::msg(format!("invalid Windows task state: {value}")))
            }
        };
        Ok(ServiceStatus {
            installed: state != ServiceState::NotInstalled,
            state,
            definition_path: None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn scheduler_states_are_not_localized() {
            for (output, installed, state) in [
                ("-1\r\n", false, ServiceState::NotInstalled),
                ("0", true, ServiceState::Unknown),
                ("1", true, ServiceState::Stopped),
                ("2", true, ServiceState::Unknown),
                ("3", true, ServiceState::Stopped),
                ("4\r\n", true, ServiceState::Running),
            ] {
                let status = task_status_from_output(output).unwrap();
                assert_eq!(status.installed, installed);
                assert_eq!(status.state, state);
            }
            for invalid in ["", "Running", "En cours", "5", "access denied"] {
                assert!(task_status_from_output(invalid).is_err());
            }
        }
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
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
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

#[cfg(test)]
mod encoding_tests {
    use super::utf16le_with_bom;

    #[test]
    fn encodes_windows_task_xml_as_utf16le_with_bom() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><Task>José 🚀</Task>";
        let encoded = utf16le_with_bom(xml);

        assert_eq!(&encoded[..2], &[0xff, 0xfe]);
        assert_eq!(encoded.len(), 2 + xml.encode_utf16().count() * 2);

        let decoded = String::from_utf16(
            &encoded[2..]
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(decoded, xml);
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
