---
title: Updates and troubleshooting
description: Update a remote daemon and resolve connection, authentication, and agent installation problems.
---

## Update a remote host

Managed daemons use the same signed update lifecycle whether the desktop
connects locally or over WebSocket. Use **Check for updates** in the desktop
while connected to that host. The single update dialog includes the application
and the connected daemon, and identifies the remote host before you confirm. The host's supervisor performs activation
and rollback; the desktop retries status over the remote connection while the
listener restarts. It never reads the laptop's local journal for a remote update.

The headless installer reuses an existing verified current release. Rerun it to
configure access, install runtimes, or migrate an old remote layout; it does not
silently switch a running service to another binary. Managed updates require the
standard running user service. A foreground/custom daemon can report updates as
unavailable; install the standard service on its host with
`amarcode-daemon bootstrap install` when appropriate.

Restarting the daemon interrupts active agent processes and terminal sessions. A separate `amarcode-daemon tunnel` connector stays independent
of the daemon, although a new connector run gets a new public hostname.

## Update a local installation

Use **Check for updates** in the desktop. One dialog lists the available application and background service releases. The connected daemon updates first, then the application installs and restarts. If only one component has an update, only that component changes. Updating the daemon interrupts active agent turns; saved chats remain available.

Daemon updates verify the signed artifact, activate the candidate, and check its exact version and protocol before committing. Failed activation attempts restore the verified previous release and stop the combined update before the app installer starts. If application installation fails after the daemon succeeds, retrying continues with the application. Check failures and services that cannot update are reported separately; they are never presented as an up-to-date result.

Managed daemon self-updates currently target supported Linux and Windows services with standard configuration. Custom transports, addresses, data paths, and development binaries can report updates as unavailable.

## Installer reports a GitHub rate limit

The installer downloads the daemon from Amarcode's registry and resolves its
connection helper and agent runtimes through GitHub's release API. A shared
outbound IP can exhaust GitHub's unauthenticated allowance even when your own
installation made only a few requests. Reinstalling the daemon does not reset
that limit.

The installer uses an exported `GH_TOKEN` or `GITHUB_TOKEN` for GitHub API
requests. `GH_TOKEN` takes precedence; tokens are not saved or sent to Amarcode's
registry, binary download hosts, or cross-host redirects. In Codespaces, an
existing exported `GITHUB_TOKEN` can be used automatically. Otherwise, export
a GitHub token in your shell before running the installer, or wait for the
rate-limit reset time shown in the error and retry.

An invalid token produces a separate authentication error. Refresh it or unset
it before retrying. Checksum and signature verification remain enabled.

## The WebSocket connection fails

Check these in order:

1. The host’s `amarcode-daemon --remote` command is still running, or its service and separate `amarcode-daemon tunnel` helper are both running.
2. The URL is from the current run and ends in `/rpc`.
3. You entered the access token separately, without query parameters in the URL.
4. The desktop and daemon use the same protocol version.
5. The host and desktop have network access to the tunnel endpoint.

An old quick-tunnel URL cannot be reused after the hostname changes.

## Containers report Running but the tunnel is refused

A container can have an old `amarcode-daemon.service` file without a working
systemd user manager. Older Amarcode releases trusted a successful exit from
`systemctl`, including wrappers that only print an unsupported-systemd message.
Current releases require the actual `active` state before reporting Running.

`amarcoded tunnel` checks the authenticated listener itself. A connection-refused
error means that listener is not reachable. In Codespaces or containers without
systemd, [start `amarcoded --remote` inside tmux](/docs/remote/setup/#codespaces-and-containers-without-systemd).
Install `tmux` first if the shell reports `command not found`. A tunnel command
does not start a daemon, and a service definition is not proof of a live process.

## Remote terminal will not connect

Run `amarcoded ssh` from an interactive terminal on your own machine. Enter the
current remote daemon URL and paste its access token when prompted, or provide a
private token file with `--token-file`.

- **Unauthorized:** verify the remote host’s token. The local daemon’s token may be different.
- **Not found:** update the remote daemon to a release supporting `/terminal`.
- **Connection refused or timeout:** confirm the host’s daemon and connection helper are still running and use the current URL.
- **Token file error:** fix that file or unset `AMARCODE_WS_TOKEN_FILE` to use the hidden token prompt.
- **Disconnected:** the shell closes with its connection. Start a new terminal session; use a host-side multiplexer for persistent work.

Remote access uses TLS and daemon token authentication. There is no SSH host,
identity-file, or SSH-port form in the app. See [Remote terminal](/docs/remote/ssh/).

## Authentication is rejected

Read the token from the host's `access-token` file and re-enter it. For manually configured hosts, `AMARCODE_WS_TOKEN_FILE` must point to a valid private token file. On Unix, its permissions must exclude group and other access, such as mode `0600`.

If the desktop says authentication is not enforced, install a daemon release with remote support and configure the token on that host. The desktop intentionally rejects an unauthenticated remote daemon.

## An agent will not install

Check `data/daemon.log` inside the remote data directory. Installation failures include a bounded tail of package-command output.

Verify outbound network access and agent platform support. If the installation predates managed Bun and uv support, rerun the current installer and restart remote mode.

## The port is already in use

The shared daemon uses local TCP port `43821` and authenticated loopback
WebSocket port `43721`. If its service is running, restart it instead of starting
a second foreground process. Stop the old `amarcode-remote.service` before
migrating an earlier installation. A database ownership error means another
process owns the same instance; do not bypass its lock or select a new database
to hide that error.

## Share a useful issue report

Include your operating system, desktop and daemon versions, the exact error, and relevant log excerpts. Remove access tokens, API keys, private prompts, and sensitive project paths before posting to [GitHub issues](https://github.com/amar-jay/amarcode/issues).
