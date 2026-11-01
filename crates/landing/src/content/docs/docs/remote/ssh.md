---
title: Remote terminal
description: Open a shell on the machine that runs your daemon.
---

`amarcoded ssh` opens a shell on a remote Amarcode machine. It does not start a daemon, and it does not need an SSH server.

## Open a remote shell

[Start the daemon](/docs/remote/setup/#start-and-connect) on the remote machine and copy its URL and token.

On your own machine:

```sh
amarcoded ssh
```

Enter the URL and paste the token. The token is hidden and is not saved.

You can pass the URL instead:

```sh
amarcoded ssh wss://your-host/rpc
```

Commands run as the daemon’s user. Type `exit` or press Ctrl+D to leave.

## Open graphical X11 applications

X11 forwarding is enabled automatically when a local X server and `xauth`
are available on your own machine:

```sh
amarcoded ssh wss://your-host/rpc
```

Run an X11 application from that shell to show its window locally. Both client
and remote daemon need a version supporting X11 forwarding.

This is trusted X11 forwarding: connect only to hosts and applications you
trust with access to your display. Use `amarcoded ssh --no-x11` to disable it.
Headless terminals connect without forwarding. Native Wayland applications are not forwarded;
use their X11 backend. Forwarding ends when the shell disconnects.

## Use a token file

```sh
amarcoded ssh wss://your-host/rpc --token-file /absolute/path/to/remote-token
```

On Unix, run `chmod 600` on that file. `AMARCODE_WS_TOKEN_FILE` works the same way. Without either, the command asks you to paste the token.

## Connection lifetime

Closing the shell does not stop the daemon. To keep a shell across a disconnect, install `tmux` on the remote machine and run:

```sh
tmux new-session -A -s work
```

Connect again with `amarcoded ssh` and run that same command to return. Use the current URL if it changed.

In a container, also keep the daemon itself in `tmux`. See [containers without systemd](/docs/remote/setup/#codespaces-and-containers-without-systemd).
