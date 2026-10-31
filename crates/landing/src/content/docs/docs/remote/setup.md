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

This prepares remote access and prints an access token. It does not start the daemon.

If `amarcoded` is not found, open a new terminal or use the command path the installer printed. `amarcode-daemon` is the same command.

## Start and connect

```sh
amarcoded --remote
```

Copy the URL and access token. In the desktop, open **Settings → Connection**, choose **Remote daemon**, paste both, and select **Verify and apply**.

Leave this command running. Ctrl+C stops the daemon. If you start it again and the URL changes, update the connection settings.

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

`tmux` survives a closed terminal. It does not survive stopping the container or rebooting the machine. If the daemon exits, start it again and copy the new URL.

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
