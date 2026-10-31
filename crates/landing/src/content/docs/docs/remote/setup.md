---
title: Set up a remote host
description: Install Amarcode on Linux and connect your desktop using its secure remote URL.
---

Install the daemon on a Linux machine to work with its projects from Amarcode. Files, conversations, agents, and credentials stay on that host.

## Before you start

You need Linux, Bash, Python 3, OpenSSL with Ed25519 support, and internet access. The installer sets up the daemon and the tools needed to install agents.

## Install or reuse the daemon

On the remote host, run:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash
```

Remote access is included. The installer creates an access token and prints the next steps; it does not start the daemon.

Open a new terminal if `amarcoded` is not found, or use the PATH command printed by the installer. `amarcode-daemon` is an alternative name for the same command.

## Start and connect

Run:

```sh
amarcoded --remote
```

Copy the connection URL and access token. In the desktop, open **Settings → Connection**, choose **Remote daemon**, and select **Verify and apply** after entering both.

Keep this command running while you work. Ctrl+C stops the daemon and interrupts active agents. A new connection-helper session may print a different URL; update the app's connection settings if it changes.

See [connect from the desktop](/docs/remote/connect/) for choosing a remote project.

:::caution[Keep the token private]
Anyone with the access token can access your daemon and perform operations on its host.
:::

## Codespaces and containers without systemd

Run the daemon inside `tmux` to keep it running when you close your terminal.

On Ubuntu or Debian, install `tmux` if needed:

```sh
sudo apt-get update
sudo apt-get install -y tmux
```

Start a session and run the daemon inside it:

```sh
tmux new-session -s amarcode
amarcoded --remote
```

Copy the URL and token, then press **Ctrl+B**, release both keys, and press **D** to detach. Return with:

```sh
tmux attach-session -t amarcode
```

If the daemon exits, start it again and copy its new URL. `tmux` keeps work running across terminal disconnects, but not container stops, suspension, or host reboots.

## Use an existing service

On a host with a working systemd user manager, install a persistent service and open remote access with:

```sh
amarcoded bootstrap install
amarcoded tunnel
```

If the service was running before remote access was configured, run `amarcoded restart` before starting the tunnel.

Keep the tunnel command running. Stopping it closes remote access while the daemon keeps running. The tunnel command requires an already-running daemon.

## Loopback-only setup

For access through a private network path or tunnel you manage:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash -s -- --no-tunnel
amarcoded --remote --no-tunnel
```

The authenticated listener uses `127.0.0.1:43721`. Make it reachable through your chosen network path, then connect using its URL and access token.

## Installation layout

The default Linux installation is `${XDG_DATA_HOME:-$HOME/.local/share}/com.amarcode.desktop/`. Its `data/` directory contains conversations, attachments, agent installations, credentials, and logs. Project folders stay where you created them.

The desktop and command-line tools share one installation per user on each host.

## Migrate an existing remote installation

Back up your data and stop the old daemon before rerunning the installer. It can migrate an older remote installation while preserving conversations and the access token.

If the installer reports conflicting data directories or tokens, keep both backups and resolve the conflict before continuing. Separate conversation databases are not merged automatically.
