---
title: Set up a remote host
description: Install Amarcode on Linux and connect from the desktop.
---

Install the daemon on a Linux machine when you want its projects in Amarcode. Files, chats, agents, and credentials stay on that machine.

## Before you start

You need Linux, Bash, Python 3, and internet access.

## Install

On the remote machine:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash
```

This prepares remote access. It does not start the daemon.

Use the start command printed by the installer; it works in your current terminal. If `amarcoded` is not found yet, open a new terminal or use that full command path. `amarcode-daemon` is the same command.

## Start and connect

Public remote access uses a persistent named hostname. Obtain an enrollment code
from your Amarcode operator and supply `AMARCODE_TUNNEL_ENROLLMENT_CODE` for the
first start. Your private installation identity is saved on this machine; later
starts reuse the same hostname without requiring the code. The operator must
configure and deploy the named-tunnel allocator before first enrollment.


```sh
amarcoded --remote
```

To start with a project folder selected:

```sh
amarcoded --remote /path/to/project
```

Copy the Remote URL (a stable `wss://m-<id>.<operator-domain>/rpc` address) and access token. In the desktop, open **Settings → Connection**, choose **Remote daemon**, paste both, and select **Verify and apply**.

Leave this command running. Ctrl+C stops the daemon. Starting it again reuses the same URL. Offline machines retain their allocation.

See [connect from the desktop](/docs/remote/connect/).

:::caution[Keep the token private]
Anyone with the token can use the daemon on that machine.
:::

## Codespaces and containers without systemd

A container often has no working service manager. `amarcoded status` can look installed while nothing is listening. Run the daemon in `tmux` so it keeps going after you close the terminal.

```sh
sudo apt-get update
sudo apt-get install -y tmux
tmux new-session -s amarcode
amarcoded --remote
```

Copy the URL and token. Detach with **Ctrl+B**, then **D**. Come back with:

```sh
tmux attach-session -t amarcode
```

`tmux` survives a closed terminal. It does not survive stopping the container or rebooting the machine. If the daemon exits, start it again to reconnect using the same URL.

## Use a service

On a machine with a working systemd user manager:

```sh
amarcoded bootstrap install
amarcoded tunnel
```

If the daemon was already running, run `amarcoded restart` before `amarcoded tunnel`. The tunnel command needs a running daemon. Stopping the tunnel closes remote access and leaves the daemon running.

## Loopback-only setup

Use this when you already have your own private network path:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash -s -- --no-tunnel
amarcoded --remote --no-tunnel
```

The listener is `127.0.0.1:43721`. Reach it through your own path, then connect with its URL and token.


## Remove a named allocation

Stop the connector and run:

```sh
amarcoded tunnel --rm
```

This revokes the connector, removes its public DNS record, and frees installation
capacity. The old hostname is never assigned to another installation. Run this
before purging local installation data. Keep `named-installation.json` private
and preserve it across normal restarts and daemon upgrades.

Public remote access uses named tunnels exclusively. The private loopback mode
above does not require allocator enrollment.
