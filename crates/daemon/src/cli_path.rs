//! Per-user terminal command registration. Never modifies the machine PATH.
use std::{fs, path::Path};

use crate::{Error, Result};

fn bin_dir() -> Result<std::path::PathBuf> {
    crate::update::release_root()
        .map(|root| root.join("bin"))
        .map_err(Error::msg)
}

pub fn install(executable: &Path) -> Result<()> {
    let bin = bin_dir()?;
    fs::create_dir_all(&bin)?;
    install_command(&bin, executable)?;
    configure_path(&bin, false)
}

pub fn uninstall() -> Result<()> {
    let bin = bin_dir()?;
    configure_path(&bin, true)?;
    for name in ["amarcode-daemon", "amarcoded"] {
        match fs::remove_file(bin.join(name)) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn install_command(bin: &Path, executable: &Path) -> Result<()> {
    for name in ["amarcode-daemon", "amarcoded"] {
        let temporary = bin.join(format!(".command-{}", uuid::Uuid::new_v4()));
        std::os::unix::fs::symlink(executable, &temporary)?;
        let result = fs::rename(&temporary, bin.join(name));
        let _ = fs::remove_file(temporary);
        result?;
    }
    Ok(())
}

fn configure_path(bin: &Path, remove: bool) -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| Error::msg("home directory unavailable"))?;
    let zdot = std::env::var_os("ZDOTDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.clone());
    let config =
        dirs::config_dir().ok_or_else(|| Error::msg("configuration directory unavailable"))?;
    linux_profiles(&home, &zdot, &config, bin, remove)
}

fn linux_profiles(home: &Path, zdot: &Path, config: &Path, bin: &Path, remove: bool) -> Result<()> {
    let login = [".bash_profile", ".bash_login"]
        .into_iter()
        .find(|name| home.join(name).exists())
        .unwrap_or(".profile");
    let mut profiles = vec![
        home.join(".profile"),
        home.join(login),
        home.join(".bashrc"),
        zdot.join(".zshrc"),
    ];
    if remove {
        profiles.extend([home.join(".bash_profile"), home.join(".bash_login")]);
    }
    profiles.sort();
    profiles.dedup();
    for path in profiles {
        edit_profile(&path, &posix_block(bin)?, remove)?;
    }
    let fish = format!("# >>> Amarcode daemon PATH >>>\nif not contains -- {} $PATH\n    set -gx PATH $PATH {}\nend\n# <<< Amarcode daemon PATH <<<\n", shell_quote(bin)?, shell_quote(bin)?);
    edit_profile(
        &config.join("fish/conf.d/amarcode-daemon.fish"),
        &fish,
        remove,
    )
}

fn shell_quote(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| Error::msg("PATH directory is not Unicode"))?;
    if path.contains(['\r', '\n', ':']) {
        return Err(Error::msg("PATH directory contains a newline or colon"));
    }
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}

fn posix_block(bin: &Path) -> Result<String> {
    // Quote the entire path as one shell word, including glob metacharacters.
    let bin = shell_quote(bin)?;
    Ok(format!("# >>> Amarcode daemon PATH >>>\ncase \":$PATH:\" in\n  *:{bin}:*) ;;\n  *) export PATH=\"$PATH\":{bin} ;;\nesac\n# <<< Amarcode daemon PATH <<<\n"))
}

fn edit_profile(path: &Path, block: &str, remove: bool) -> Result<()> {
    let original = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let begin = "# >>> Amarcode daemon PATH >>>\n";
    let end = "# <<< Amarcode daemon PATH <<<\n";
    let mut updated = original.clone();
    if let Some(start) = updated.find(begin) {
        let finish = updated[start..]
            .find(end)
            .ok_or_else(|| Error::msg("incomplete Amarcode PATH block"))?
            + start
            + end.len();
        updated.replace_range(start..finish, "");
    }
    if !remove {
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(block);
    }
    if updated != original {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, updated)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_are_idempotent_and_command_follows_updates() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path();
        let bin = home.join("a ' bin");
        fs::create_dir(&bin).unwrap();
        fs::write(home.join(".bash_profile"), "# keep me\n").unwrap();
        let executable = home.join("version1");
        fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        install_command(&bin, &executable).unwrap();
        for _ in 0..2 {
            linux_profiles(home, home, &home.join("config"), &bin, false).unwrap();
        }
        let profile = fs::read_to_string(home.join(".bash_profile")).unwrap();
        assert_eq!(profile.matches("# >>> Amarcode").count(), 1);
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(". \"$1\"; amarcoded 'two words' --version")
            .arg("sh")
            .arg(home.join(".profile"))
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"two words\n--version\n");
        let next = home.join("version2");
        fs::copy(&executable, &next).unwrap();
        install_command(&bin, &next).unwrap();
        assert_eq!(fs::read_link(bin.join("amarcode-daemon")).unwrap(), next);
        assert_eq!(fs::read_link(bin.join("amarcoded")).unwrap(), next);
        linux_profiles(home, home, &home.join("config"), &bin, true).unwrap();
        assert_eq!(
            fs::read_to_string(home.join(".bash_profile")).unwrap(),
            "# keep me\n"
        );
    }
}
