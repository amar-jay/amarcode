---
title: Set up a remote host
description: Install one managed Amarcode daemon on Linux and enable authenticated SSH or optional Cloudflare access.
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
curl -fsSL https://amarcode.amarjay.com/install.sh -o install.sh
less install.sh
bash install.sh
```

The installer verifies the daemon and managed Bun and uv runtimes. If a managed
daemon is already installed, it verifies and reuses its current release instead
of replacing a running service. The current release record and immutable binary
layout are the same ones used by desktop bootstrap and the managed updater.

It creates a private access token if one is missing and enables authenticated
loopback access in `remote.json`. Cloudflare is optional and is not downloaded
by default. The installer starts no service or tunnel unless you pass `--start`;
that option runs the shared daemon in the foreground.

The stable command lives in `daemon/bin/amarcode-daemon` under the instance root.
The installer adds that directory to Bash, Zsh, and Fish startup configuration.
Open a new terminal or use the PATH command it prints. `--bin-dir` creates an
additional alias pointing to the stable command, so it follows managed updates.

## Start the shared instance

For a foreground daemon:

```sh
amarcode-daemon run
```

For a persistent per-user service on a host with a systemd user manager:

```sh
amarcode-daemon bootstrap install
```

If the existing service is already running, apply the newly enabled access with:

```sh
amarcode-daemon restart
```

One process serves local TCP on `127.0.0.1:43821` and authenticated WebSocket on
`127.0.0.1:43721`. Both connections share the same live sessions, approvals, and
database. Do not start a second foreground daemon while the service is running.
Restarting the daemon interrupts active agent processes; closing a desktop
connection does not.

Follow [Connect over SSH](/docs/remote/ssh/) to reach that loopback WebSocket
listener through the desktop's SSH tunnel. The desktop connects to an already
running daemon; it does not provision the host over SSH.

To change access on an existing installation without rerunning the installer:

```sh
amarcode-daemon access enable
amarcode-daemon restart
```

`access disable` disables the additional listener after the next restart. It
preserves the database and token. Access settings survive managed daemon updates
and rollback. The standard listener is fixed to remote IPv4 loopback for SSH;
`AMARCODE_WS_ADDR` or `remote.json`'s `listen_addr` changes its port. The local
TCP address remains controlled separately by `AMARCODE_DAEMON_ADDR`.

## Optional Cloudflare access

Install the connector explicitly:

```sh
bash install.sh --with-cloudflare
```

Start or restart the shared daemon as above, then attach a tunnel:

```sh
amarcode-daemon tunnel
```

The command authenticates to the running WebSocket listener before starting
Cloudflare. It prints a `wss://…/rpc` URL and the access token for
[the desktop's WebSocket connection settings](/docs/remote/connect/).
Ctrl+C stops the connector and leaves the daemon and its work running. If the
connector fails, the daemon also remains running. Quick-tunnel hostnames change
between runs; the token and conversations stay on disk.

:::caution[Keep the token private]
The access token authorizes access to your daemon and its host operations.
Do not put it in a URL or share it with others.
:::

`--remote` remains a foreground convenience for starting the shared daemon and
Cloudflare together; `--remote --no-tunnel` starts its authenticated loopback
access without Cloudflare. Prefer the separate `run` / service and `tunnel`
commands when work should outlive the connector.

## Installation layout

On Linux the shared instance root is
`${XDG_DATA_HOME:-$HOME/.local/share}/com.amarcode.desktop/`:

| Path | Purpose |
| --- | --- |
| `data/` | Chats, attachments, agent installations, credentials, and logs |
| `daemon/` | Verified releases, current release record, update journal, and stable command |
| `access-token` | Private remote bearer credential |
| `remote.json` | Optional access settings and runtime/connector locations |
| `runtimes/` | Managed Bun and uv tools |
| `cloudflared/` | Optional connector |

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
