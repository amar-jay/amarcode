---
title: Set up a remote host
description: Install Amarcode on Linux and connect your desktop using its secure remote URL.
---

A remote connection uses the daemon on the selected host. The desktop and
headless installer share one daemon installation, release store, and database
per user on that host. Project files, chats, agent processes, and provider
credentials stay there. Connecting from another machine does not create a second
instance or copy its database.

## Before you start

The installer needs Linux, Bash, Python 3, OpenSSL with Ed25519 support, and
outbound internet access. You do not need root, a systemd user session, or
preinstalled Rust, Bun, or uv. A signed release advertising `unified-instance`
support must be available for the host architecture; older releases are rejected.

If both the old remote daemon and a desktop daemon contain data, see
[migrate an existing remote installation](#migrate-an-existing-remote-installation)
before continuing.

## Install or reuse the daemon

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash
```

The installer verifies the daemon and managed Bun and uv runtimes. If a managed
daemon is already installed, it verifies and reuses its current release instead
of replacing a running service. The current release record and immutable binary
layout are the same ones used by desktop bootstrap and the managed updater.

Remote access is included by default. The installer sets up the connection
helper and creates a private access token if one is missing. You do not need to
choose or configure a tunnel provider.

Installation does not start the daemon. To install and start a foreground remote
session in one command, use:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash -s -- --start
```

The stable commands `amarcoded` and `amarcode-daemon` share the same managed
executable in `daemon/bin/` under the instance root.
The installer adds that directory to Bash, Zsh, and Fish startup configuration.
Open a new terminal or use the PATH command it prints. `--bin-dir` creates an
additional alias pointing to the stable command, so it follows managed updates.

## Start and connect

On the remote host, run:

```sh
amarcode-daemon --remote
```

Wait for the connection URL and access token to appear. In the desktop, open
**Settings → Connection**, choose **Remote WebSocket**, and enter the URL and
token. See [connect from the desktop](/docs/remote/connect/) for the full steps.

Keep the command running while you use it. Ctrl+C stops this foreground daemon
and its connection helper, interrupting active agent processes. Closing the
desktop connection leaves the host command and its work running. The connection
URL changes each time you start a new helper; your token and conversations are
preserved.

If the daemon already runs as a service, use the
[existing-service workflow](#use-an-existing-service) below instead of starting a
second process.

:::caution[Keep the token private]
The access token authorizes access to your daemon and its host operations.
Do not put it in a URL or share it with others.
:::

## Use an existing service

For a persistent daemon on a host with a systemd user manager:

```sh
amarcode-daemon bootstrap install
amarcode-daemon tunnel
```

If the service was already running before installation enabled remote access,
apply the new settings with `amarcode-daemon restart`, then run
`amarcode-daemon tunnel`. The connection helper attaches to the existing daemon
and prints the connection URL and token. It authenticates to that daemon before
opening remote access. Ctrl+C stops the helper and leaves the daemon and its
work running. If the helper fails, the daemon also remains running.

One process serves local TCP and authenticated remote WebSocket connections.
Both share the same live sessions, approvals, and database. Restarting that
process interrupts active agent work, so restart only when appropriate.

To enable remote connections on an existing daemon without rerunning the
installer, use `amarcode-daemon access enable`, then restart it.
`access disable` disables remote connections after the next restart while
preserving the database and token. Access settings survive managed updates and
rollback.

## SSH-only setup

If you prefer connecting through your own SSH access, opt out of the included
public connection helper:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash -s -- --ssh-only
```

This installs the same shared daemon and agent runtimes, and enables its
private authenticated listener without downloading the public connection helper.
Start it with `amarcoded ssh`, or use the existing user service.
`--ssh-only --start` starts the foreground daemon immediately after installation.
Follow [Connect over SSH](/docs/remote/ssh/) to configure the desktop's tunnel.
The desktop connects to an existing daemon and does not provision the host.

`amarcoded ssh` creates or reuses the private token and starts authenticated
loopback access. The older `amarcode-daemon --remote --no-tunnel` spelling is
also supported. The WebSocket listener uses
`127.0.0.1:43721`; `AMARCODE_WS_ADDR` or `remote.json`'s `listen_addr` changes its
port. Local TCP uses `127.0.0.1:43821` and is configured separately through
`AMARCODE_DAEMON_ADDR`.

The included public connection helper uses Cloudflare Tunnel. Its installation
and configuration are handled automatically; the normal setup requires no
provider-specific options.

## Installation layout

On Linux the shared instance root is
`${XDG_DATA_HOME:-$HOME/.local/share}/com.amarcode.desktop/`:

| Path           | Purpose                                                                       |
| -------------- | ----------------------------------------------------------------------------- |
| `data/`        | Chats, attachments, agent installations, credentials, and logs                |
| `daemon/`      | Verified releases, current release record, update journal, and stable command |
| `access-token` | Private remote bearer credential                                              |
| `remote.json`  | Optional access settings and runtime/connector locations                      |
| `runtimes/`    | Managed Bun and uv tools                                                      |
| `cloudflared/` | Managed remote connection helper                                                            |

Project directories stay wherever you created them on the host. The application
on your laptop can have its own local daemon and database; it is a different host.

## Migrate an existing remote installation

Stop the old foreground daemon or `amarcode-remote.service`, then rerun this
installer. When the old `amarcode-remote/data` is the only populated instance,
it moves the complete data directory to the shared root and preserves the
access token. The old data path becomes an alias of the new one, so old launchers
cannot silently open a second database. Old owned command aliases are redirected
to the stable managed command. Legacy runtime and binary files are retained.

Migration refuses a database held by a running daemon, conflicting access tokens,
or a populated target data directory. If both installations contain data, both
are preserved: stop both daemons, back up the directories, and choose the instance
you want to keep before migrating. Databases are not automatically merged.
An interrupted migration is detected at startup; rerunning the installer resumes it.

Advanced isolated setups can explicitly select `AMARCODE_APPDIR` and
`AMARCODE_STORE_PATH`. Legacy `AMARCODE_REMOTE_ROOT` with `--remote` still selects
that explicit installation; it is not the default. Custom instances are outside
the standard service updater and need their own process supervision and lifecycle.
