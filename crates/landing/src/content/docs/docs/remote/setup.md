---
title: Set up a remote host
description: Install the Amarcode daemon on Linux and expose it through an authenticated Cloudflare tunnel.
---

Remote mode keeps project files, chats, agents, and provider credentials on a Linux machine. Your desktop connects to it through an authenticated WebSocket connection.

This page describes the installer and Cloudflare quick-tunnel workflow. If you
already run an authenticated WebSocket daemon and have SSH access to its host,
follow [Connect over SSH](/docs/remote/ssh/) instead. SSH connections do not require
a public daemon URL; the desktop does not provision or start the remote daemon.
For an already installed daemon, use
[`amarcode-daemon --remote --no-tunnel`](/docs/remote/ssh/#start-the-installed-daemon-without-cloudflare)
on the host to start authenticated loopback mode without Cloudflare. Remote
installation over SSH is not part of this workflow.

## Before you start

The host needs Linux, Bash, Python 3, OpenSSL with Ed25519 support, and outbound internet access. You do not need root, a systemd user session, or preinstalled Rust, Bun, or uv.

A signed daemon release must be available for the host architecture. The installer requires a release advertising authenticated WebSocket and remote CLI support; it rejects older incompatible releases.

## Run the installer

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash
```

To inspect the script first:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh -o install.sh
less install.sh
bash install.sh
```

The installer downloads and verifies the daemon, Cloudflare connector, and managed Bun and uv runtimes. It creates a private access token and links the daemon into `~/.local/bin`.

The installer adds the command directory to your Bash, Zsh, and Fish startup configuration. Open a new terminal, or apply the command printed by the installer in your current shell:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

PATH setup is idempotent and preserves existing configuration. A custom `--bin-dir` is added in the same way. The installer does not start the daemon or tunnel unless you pass `--start`.

## Start remote mode

```bash
amarcode-daemon --remote
```

The daemon binds its loopback listener before it starts the Cloudflare connector. When the tunnel is ready, it prints a WebSocket URL and access token.

Keep the command running while you use the connection. Ctrl+C stops both the daemon and connector. If the connector fails, the daemon stops with an error.

:::caution[Keep the token private]
The access token authorizes access to your daemon and its host operations. Enter it in the desktop's access-token field. Do not put it in a URL or share it with others.
:::

## Connect your desktop

Follow [connect from the desktop](/docs/remote/connect/) using the printed `wss://…/rpc` URL and token.

## Know what changes on restart

The quick-tunnel hostname changes between runs. The access token and chat history stay on disk and are preserved when you rerun the installer.

Files live under `${XDG_DATA_HOME:-$HOME/.local/share}/amarcode-remote/`:

| Path           | Purpose                                                        |
| -------------- | -------------------------------------------------------------- |
| `access-token` | Private bearer credential                                      |
| `remote.json`  | Connector and runtime locations                                |
| `data/`        | Chats, attachments, agent installations, credentials, and logs |
| `releases/`    | Verified daemon releases                                       |
| `runtimes/`    | Managed Bun and uv tools                                       |

Project directories stay wherever you created them on the host.
