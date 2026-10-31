# Desktop uninstall and shared daemon ownership

Removing the desktop application removes its installed executable, bundled
bootstrap, shortcuts, and application registration. It preserves the independent
per-user daemon, service registration, current release, update journal, database,
access token, and access configuration. Active work and other local or remote
clients keep using that daemon. Application updates preserve it too.

The managed service runs an immutable executable under the user's daemon release
store, rather than the bootstrap inside the desktop's installation directory.
The desktop is a client of that instance; uninstalling a client does not own its
server's lifecycle.

## Explicit local daemon removal

Open **Settings → General → Remove local daemon and data**, type
`DELETE LOCAL DAEMON DATA`, and confirm. The dialog names this computer as the
target and explains that all clients of its shared daemon will be interrupted.
The native `remove_local_daemon` IPC also checks the exact phrase before any
service or filesystem operation. A generic application uninstall does not invoke
this command.

The explicit operation:

1. serializes removal against this application's bootstrap/install operations;
2. asks the local daemon lifecycle executable to stop and unregister the service;
3. verifies service absence and acquires database ownership before deleting data;
4. removes daemon credentials, access settings, and cached daemon releases; and
5. leaves the desktop, its preferences, and saved remote connections available.

A foreground daemon can own the database without a native service. Removal
refuses to delete its live database or attachments: stop that process and retry.
A durable `daemon-cleanup-pending-v1` phase marker makes cache removal safe to
retry after an interrupted operation.

This operation always targets the standard local instance, even when the desktop
is connected to another host. It never sends a remote purge request. Workspace
project directories and UI preferences/WebView storage are preserved. A local
connection will need a daemon installed again; a connection to another host can
continue using that host's daemon.

## Native uninstallers

- **Windows NSIS:** the pre-uninstall hook never stops or unregisters the daemon
  and clears `DeleteAppDataCheckboxState`. Tauri's generic deletion branch would
  otherwise recursively remove the bundle-ID data directories, including the
  shared daemon. If a user selects **Delete app data**, the uninstaller explains
  the separate daemon-removal flow and preserves that directory. Silent uninstall
  and `/UPDATE` preserve it as well.
- **Windows MSI:** removal affects desktop-owned installation files; no custom
  daemon-removal action is registered.
- **Linux DEB/RPM and AppImage:** package/application removal does not remove
  daemon state from user home directories or unregister the user service.
- **macOS:** removing the application bundle leaves the shared daemon alone.

Daemon removal is optional and independent of desktop removal. Run the explicit
in-app action before uninstalling if that daemon should be removed too.

Service-only removal remains available as `amarcode-daemon uninstall` and
preserves conversations and credentials. Full daemon data removal is
`amarcode-daemon purge --confirm-data-loss`. The latter interrupts clients and
permanently deletes the local instance's data; it is never run automatically by
a desktop uninstaller. Use the managed executable's full path if the command is
not on PATH.

## Regression checks

Run `cargo test -p amarcode --lib` and `cargo test -p amarcode-daemon cleanup::tests --lib`
for native confirmation and database ownership guards.

On Windows with NSIS installed, run:

```powershell
powershell -NoProfile -File scripts/test-windows-uninstall.ps1
```

The script compiles the real hook into an isolated uninstaller and exercises
normal removal, selected data deletion, and update modes. It removes only fixture
desktop files and checks preservation of the fixture's independent daemon binary,
release record, SQLite/WAL, access token, access settings, and preferences. It
creates no real service or user application-data files. The release workflow runs
these checks before publishing.
