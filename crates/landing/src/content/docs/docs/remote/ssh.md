---
title: Remote terminal
description: Open an interactive shell on a remote machine through its authenticated Amarcode daemon.
---

Use `amarcoded ssh` from your terminal to open a shell on a remote Amarcode host.

## Open a remote shell

First [install and start the daemon](/docs/remote/setup/#start-and-connect) on the remote host and copy its URL and access token.

On your own machine, run:

```sh
amarcoded ssh
```

Enter the URL and paste the token when prompted. Token input is hidden and is not saved.

You can also pass the URL directly:

```sh
amarcoded ssh wss://your-host/rpc
```

Commands run as the daemon's user on the remote host. Type `exit` or press Ctrl+D to close the shell.

This uses your Amarcode connection; no SSH server or SSH keys are needed.

## Use a token file

For repeated connections, keep the remote host's token in a private local file:

```sh
amarcoded ssh wss://your-host/rpc --token-file /absolute/path/to/remote-token
```

On Unix, restrict the file with `chmod 600 /absolute/path/to/remote-token`.

You can set `AMARCODE_WS_TOKEN_FILE` instead. If neither is supplied, the command prompts for a token.

## Connection lifetime

Disconnecting closes the shell. To keep work running, install `tmux` on the remote host and run:

```sh
tmux new-session -A -s work
```

After a disconnect, run `amarcoded ssh` again and enter the same `tmux` command to return to your work. Use the current daemon URL if it has changed.

Closing a remote terminal leaves the daemon and unrelated agent work running. See [keeping the daemon running in a container](/docs/remote/setup/#codespaces-and-containers-without-systemd).

## Loopback-only deployment

For a connection through a network path you manage, see [loopback-only setup](/docs/remote/setup/#loopback-only-setup).
