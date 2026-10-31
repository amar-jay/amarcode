---
title: Updates and troubleshooting
description: Update Amarcode and fix connection or install problems.
---

## Update

Choose **Check for updates**. One dialog covers the app and the daemon you are connected to. On a remote connection, check the machine name before you continue.

A daemon update stops active agent work and remote shells. Saved chats remain. If it fails, read the message and retry.

A foreground daemon does not update itself. Stop it and start it again after you install the new version.

## The connection fails

1. The daemon is still running. In a container, use the [tmux steps](/docs/remote/setup/#codespaces-and-containers-without-systemd).
2. The URL is the current one and includes `/rpc`.
3. The token is from that same machine.
4. The app and daemon are current.

Starting remote access again can print a new URL. Paste that URL into **Settings → Connection**.

## Authentication is rejected

Copy the token again from the remote machine. A token from your laptop is not the remote token.

If you use a token file, run `chmod 600` on it.

## Connection refused

The daemon is not listening. A service file or a finished install does not mean the process is running. Start it with `amarcoded --remote`, or with `amarcoded tunnel` only when `amarcoded status` shows it is actually running.

## The port is already in use

A daemon is already running. Use `amarcoded restart` instead of starting a second one.

## The remote shell fails

Run `amarcoded ssh` from an interactive terminal, with the current URL and token.

- **Unauthorized:** use that machine’s token.
- **Not found:** update the remote daemon.
- **Timeout or connection refused:** the daemon or its remote access is not running.
- **Token file error:** fix the path, or unset `AMARCODE_WS_TOKEN_FILE` and paste the token.
- **Disconnected:** connect again and reattach `tmux` if you were using it.

## The installer says GitHub rate limit

Wait until the time in the error, then run the installer again. If you have `GH_TOKEN` or `GITHUB_TOKEN` set, the installer uses it for the download lookup.

## An agent will not install

Confirm the machine has internet access and retry from the agent picker.
