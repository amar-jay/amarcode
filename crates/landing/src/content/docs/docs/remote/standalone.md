---
title: Keep it running
description: Keep a foreground or service-managed remote daemon running independently of the Amarcode window.
---

The daemon can run on a host that has no desktop app or graphical session. Your
laptop connects to it when needed; the remote process owns the chats and agents.

## Run in the foreground

First [install on the host](/docs/remote/overview/). Then, on that host:

```sh
amarcoded --remote /home/me/projects/my-app
```

Replace the project path with an existing absolute directory. Omitting it leaves
you free to choose a project after connecting. Keep this command running and copy
the Remote URL and token it prints.

- Closing a client window leaves the daemon running.
- Disconnecting a remote shell closes that shell, not the daemon.
- **Ctrl+C in the daemon's own terminal stops the daemon and its connector.**
- Closing an ordinary SSH session can end processes launched in that terminal.
  Use `tmux` or a service if the daemon should survive it.

`--remote` normally serves the standard installation's data and adds an
authenticated remote listener. It does not create a separate history merely
because the desktop is closed.

## Keep it running with tmux

Install `tmux` if needed (`sudo apt-get install tmux` on Debian or Ubuntu).
On the remote host:

```sh
tmux new-session -s amarcode
amarcoded --remote /home/me/projects/my-app
```

Detach with **Ctrl+B**, then **D**. To return:

```sh
tmux attach-session -t amarcode
```

To stop the daemon, attach and press **Ctrl+C**. `tmux` survives a disconnected
terminal, but not a host reboot, suspended Codespace, or stopped container.
Restart the daemon when that host starts again.

For a shell opened _through_ Amarcode, use a separate tmux session inside that
shell if you want its commands to survive a client disconnect. See
[remote terminal lifetime](/docs/remote/ssh/#connection-lifetime).

## Use an existing service

A native service is useful on machines with a working user service manager:

```sh
amarcoded bootstrap install
amarcoded tunnel
```

The first command installs/ensures the standard daemon service. The second
attaches public access to its already-running authenticated listener. Keep the
`tunnel` command running, for example in tmux or an administrator-managed service.
Stopping that command stops the connector and leaves the daemon service running.

Use `amarcoded status`, `amarcoded restart`, and `amarcoded stop` for the
**registered service**. `status` does not report arbitrary foreground instances.
A container may have an installed service definition without a running service
manager; use foreground mode there.

Do not start a foreground daemon alongside a service using the same data directory
or port. Stop the existing instance first. Do not remove its data just to free a
port.

## Restart without losing the installation

Restart with the same root, data directory, identity files, and security settings.
The desktop can reconnect to its existing chats. A named tunnel normally reuses
its hostname; after inactivity cleanup, recovery is subject to available capacity.

Keep the data directory, access token, and installation identity private. Do not
delete them during a routine restart. If your administrator supplied security
settings, keep using them when restarting.

<details>
<summary>Advanced: separate instances</summary>

## Keep a genuinely separate instance

Most people should use the standard installation above. If you need a separate
history, data directory, token, and listener, set `AMARCODE_REMOTE_ROOT` explicitly.
The following Linux example uses loopback access and a different port, avoiding
public tunnel enrollment:

```sh
export AMARCODE_REMOTE_ROOT="$HOME/.local/share/amarcode-isolated"
export AMARCODE_APPDIR="$AMARCODE_REMOTE_ROOT/data"
export AMARCODE_WS_TOKEN_FILE="$AMARCODE_REMOTE_ROOT/access-token"
export AMARCODE_DAEMON_ADDR=127.0.0.1:43731
mkdir -p "$AMARCODE_REMOTE_ROOT"
chmod 700 "$AMARCODE_REMOTE_ROOT"
if [ ! -e "$AMARCODE_WS_TOKEN_FILE" ]; then
  (umask 077; openssl rand -hex 32 > "$AMARCODE_WS_TOKEN_FILE")
fi
chmod 600 "$AMARCODE_WS_TOKEN_FILE"
amarcoded --remote --no-tunnel /home/me/projects/my-app
```

OpenSSL is needed for token generation in this example. Use a fresh shell without
unrelated daemon/security overrides. In particular, an unrelated
`AMARCODE_STORE_PATH` would select a different database. The command prints the
token file path. In a second terminal on that host, read the example instance's
token when you need to connect:

```sh
cat "$HOME/.local/share/amarcode-isolated/access-token"
```

Keep the token private; it is not an SSH account password.

On your own computer, forward this instance's port:

```sh
ssh -N -o ExitOnForwardFailure=yes \
  -L 127.0.0.1:43732:127.0.0.1:43731 username@remote-host
```

Connect the native desktop to `ws://127.0.0.1:43732/rpc` using **this instance's**
token. Keep both the remote daemon and the SSH forwarding process running.

An isolated public tunnel additionally needs a valid `remote.json` setup for its
root, including the installed connector path. The normal installer prepares the
standard installation, not an arbitrary isolated root. Ask your administrator to
prepare that configuration rather than copying another machine's private tunnel
identity. Loopback-only mode above does not need it.


</details>

<details>
<summary>Advanced: tunnel assignment cache and security settings</summary>

With a matching updated registry worker, the daemon privately caches the named
tunnel assignment for up to 24 hours. Restarting with the same installation,
listener port, and origin TLS settings can start the connector without another
allocation request. Expired or changed settings require enrollment again. If a
cached connector fails, the daemon refreshes the assignment once and retries.
An older registry worker continues to work but does not enable this reuse.

The cache is `named-assignment.json` beside `named-installation.json`. It contains
a connector credential: keep it private, just like the installation identity.
You can remove only the cache file while the connector is stopped to force a
fresh allocation on the next start. `amarcoded tunnel --rm` invalidates the cache
and revokes the remote installation; deleting the cache alone does not revoke it.

Treat the data directory, access token, and `named-installation.json` as private.
Do not delete them during a routine restart. E2EE, certificates, and CRL settings
apply to standalone instances too; set them in the environment that actually
launches the remote process. See [paired E2EE](/docs/remote/end-to-end/).

</details>
