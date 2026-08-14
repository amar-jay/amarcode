# Uninstall and local-data cleanup

Normal installation, repair, application updates, and daemon updates preserve
all user data. Destructive cleanup happens only after explicit user consent.

## Supported full-cleanup flow

Open **Settings → General → Remove Amarcode data**, type
`DELETE AMARCODE DATA`, and confirm. The application then:

1. serializes cleanup against daemon bootstrap/install/update;
2. asks the verified daemon lifecycle executable to stop and unregister the
   per-user service;
3. verifies service absence before deleting daemon data;
4. removes cached signed daemon releases;
5. clears WebView local/session storage; and
6. exits.

A durable `daemon-cleanup-pending-v1` phase marker makes cache cleanup safe to
retry if the desktop process is interrupted after daemon removal.

## Owned data

| Owner                                    | Linux                               | macOS                                    | Windows                                                                    |
| ---------------------------------------- | ----------------------------------- | ---------------------------------------- | -------------------------------------------------------------------------- |
| Daemon database, logs, and runtime state | `~/.amarcode`                       | `~/Library/Application Support/amarcode` | `%LOCALAPPDATA%\amarcode`                                                  |
| Downloaded daemon releases               | Tauri `app_local_data_dir()/daemon` | same API location                        | same API location                                                          |
| UI preferences/WebView storage           | Tauri WebView data                  | Tauri WebView data                       | `%APPDATA%\com.amarcode.desktop` and `%LOCALAPPDATA%\com.amarcode.desktop` |

Workspace project files are never part of cleanup.

## Native uninstallers

- **Windows NSIS:** a real uninstall always removes the `Amarcode Daemon`
  scheduled task. Selecting Tauri's **Delete app data** checkbox also deletes
  daemon data; Tauri deletes its bundle-ID application data. `/UPDATE` mode
  skips all custom cleanup, so upgrades preserve the service and data.
- **Windows MSI:** the NSIS hook does not apply. Use the in-app cleanup flow
  before MSI removal if service and data removal is required.
- **Linux DEB/RPM and AppImage:** package removal must not recursively delete a
  user home directory from a root package script. Use the in-app flow first.
- **macOS:** use the in-app flow before removing the application bundle.

Manual service-only removal remains available as `amarcode-daemon uninstall`
and preserves data. Full daemon cleanup is
`amarcode-daemon purge --confirm-data-loss`.
