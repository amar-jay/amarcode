---
title: Remote terminal
description: Open an interactive shell on a remote machine through its authenticated Amarcode daemon.
---

`amarcoded ssh` opens an interactive terminal on a machine where the Amarcode
daemon is already running. Use it from your own terminal to work directly on the
remote device. Desktop daemon connections use a remote URL and access token;
there is no separate SSH connection type to configure in the app.

## Open a remote shell

First [install and start the daemon](/docs/remote/setup/#start-and-connect) on the
remote host. Public remote access is configured by the installer. Leave the
remote daemon running and copy its URL and access token.

On your own machine, run:

```sh
amarcoded ssh
```

Enter the remote daemon URL when prompted, then paste its access token. Token
input is hidden. The token is used for this connection and is not saved.
You can also provide the URL directly:

```sh
amarcoded ssh wss://your-host/rpc
```

The command opens the remote user's shell in their home directory. Commands,
files, and processes belong to the remote machine. Interactive programs,
terminal resizing, and Ctrl+C work through a pseudo-terminal. Type `exit` or
press Ctrl+D to close the shell. The CLI returns the shell's exit status.

This is a terminal connection through Amarcode's authenticated WebSocket API,
not the OpenSSH wire protocol. It uses the existing daemon URL and token; it
does not require an SSH server, SSH keys, or a separately exposed SSH port.
The daemon runs the shell with its own operating-system account and permissions.

## Use a token file

For repeated connections, keep the remote daemon's token in a private local file:

```sh
amarcoded ssh wss://your-host/rpc --token-file /absolute/path/to/remote-token
```

`AMARCODE_WS_TOKEN_FILE` is also supported. An explicit `--token-file` takes
precedence over that environment variable. If neither is set, the command asks
you to paste a token. A configured but missing, invalid, or publicly readable
Unix token file fails instead of silently choosing another credential.

A remote token file is separate from the access token for your own local daemon.
Tokens are sent in an authorization header, never as URL parameters.
Remote connections require `wss://`; `ws://` is accepted only for loopback.

## Connection lifetime

Each terminal connection owns its own shell. Disconnecting closes that shell;
reconnecting creates a new shell. Use a host-side multiplexer such as `tmux`
when you need a terminal workload to survive disconnects.

Closing the terminal leaves the daemon and unrelated chat agent runs running.
`amarcoded ssh` does not start, install, restart, or reconfigure the remote daemon
or its public connection helper. Start the server on the host with
`amarcoded --remote`; use `amarcoded tunnel` for public access to an already
running authenticated daemon.

Both client and host need a daemon release that supports the terminal endpoint.
The CLI requires an interactive local terminal. This command does not execute a
single supplied command or automatically resume an earlier terminal session.

## Loopback-only deployment

To start the server without its public connection helper, use the advanced
[loopback-only setup](/docs/remote/setup/#loopback-only-setup). Access that loopback
listener through a network path you manage, then pass its reachable loopback URL
to `amarcoded ssh`. This server deployment option is separate from opening an
interactive shell.
