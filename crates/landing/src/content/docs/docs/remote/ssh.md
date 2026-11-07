---
title: Remote terminal
description: Open a shell on the machine that runs your daemon.
---

`amarcoded ssh` opens a shell on a remote Amarcode machine. It does not start a daemon, and it does not need an SSH server.

## Use a standard SSH client

You can use OpenSSH (`ssh`), Windows OpenSSH, PuTTY, or another SSH client
independently of Amarcode. The remote machine needs an SSH server, an SSH account,
and a reachable IP address or SSH hostname, over your network, VPN, or an
operator-configured SSH tunnel. On Ubuntu, see the
[OpenSSH server setup guide](https://ubuntu.com/server/docs/how-to/security/openssh-server/).

From your own machine:

```sh
ssh username@remote-host
```

Replace `username` with your account on the remote machine and `remote-host` with
its reachable SSH address. For a different port or a specific private key:

```sh
ssh -p 2222 -i ~/.ssh/id_ed25519 username@remote-host
```

Verify the host-key fingerprint against the remote machine on first connection.
SSH uses the account's SSH keys or password, rather than the Amarcode access token.
In a graphical client, select **SSH** and enter the same host, port, user, and key.
The daemon does not need to be running for this login.

:::note[The daemon URL is not an SSH address]
A URL such as `wss://a7c42e19.amarjay.com/rpc` serves Amarcode's WebSocket API.
The managed tunnel does not expose an SSH server or forward port 22. Passing that
hostname to `ssh`, or changing the SSH port to 443, does not provide SSH access.

For SSH over Cloudflare, your operator must configure a separate SSH route to
`sshd`. Cloudflare documents a
[standard SSH client with a cloudflared ProxyCommand](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/use-cases/ssh/ssh-cloudflared-authentication/).
That setup has its own SSH hostname and access policy; the Amarcode installer
does not create it.
:::

### Connect the desktop through SSH port forwarding

Once SSH login works, you can carry the daemon's authenticated loopback
WebSocket connection through it without a public Amarcode tunnel.
On the remote machine, [prepare loopback-only access](/docs/remote/setup/#loopback-only-setup)
and start:

```sh
amarcoded --remote --no-tunnel
```

Keep it running. In another terminal on your own machine:

```sh
ssh -N -o ExitOnForwardFailure=yes \
  -L 127.0.0.1:43722:127.0.0.1:43721 username@remote-host
```

In Amarcode, open **Settings → Connection**, select **Remote daemon**, and use
`ws://127.0.0.1:43722/rpc` with the remote daemon's access token. Select
**Verify and apply**. Keep the SSH command running while connected; Ctrl+C stops
forwarding. The separate local port `43722` avoids the usual daemon port on your
own machine. The remote destination is the remote machine's loopback listener.

For a custom SSH port or key, add the same `-p` or `-i` options used for login.
This forwards the desktop connection; ordinary SSH login still uses `ssh username@remote-host`. See the [OpenSSH client manual](https://man.openbsd.org/ssh)
for forwarding options.

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
