---
title: Host setup options
description: Enrollment, containers, services, and private-network setup for remote hosts.
---

For your first connection, follow [Get started](/docs/remote/overview/).
This reference covers additional host setup options. You do not need to read it
for a standard installation.

Install the daemon on a Linux machine when you want its projects in Amarcode. Files, chats, agents, and credentials stay on that machine.

## Before you start

You need Linux, Bash, Python 3, `curl`, OpenSSL with Ed25519 support, and internet
access. Install Git on the host for project Git operations and the agent catalog.

## Install

On the remote machine:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash
```

This prepares remote access. It does not start the daemon.

Use the start command printed by the installer; it works in your current terminal. If `amarcoded` is not found yet, open a new terminal or use that full command path. `amarcode-daemon` is the same command.

## Enrollment code

Registering a new installation requires an **enrollment code** issued by whoever
runs the tunnel service. Ask for it before you start. It is not the access token,
and it is not needed on later starts.

Pass it to the installer with `--enrollment-code`:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash -s -- --enrollment-code '<code>'
```

The installer stores it in `remote.json` under the instance directory with
`0600` permissions, so restarts, the service, and `amarcoded tunnel` keep working
without repeating the flag. To supply it another way, set
`AMARCODE_TUNNEL_ENROLLMENT_CODE` in the environment of the daemon or connector
instead; the environment takes precedence over the stored value.

| Mode                                | Enrollment code needed |
| ----------------------------------- | ---------------------- |
| First start of a new installation   | Yes                    |
| Later starts with the same identity | No                     |
| `--no-tunnel` loopback mode         | No                     |

A rejected code prints
`Named tunnel enrollment was denied; set AMARCODE_TUNNEL_ENROLLMENT_CODE to the operator enrollment code`
and returns `403` before any capacity is reserved, so retrying with the correct
code does not consume a slot. See
[troubleshooting](/docs/remote/troubleshooting/#tunnel-enrollment-is-denied).

:::caution[Keep the code private]
The code admits anyone who can reach the public installer to consume an
installation slot. Treat it like a shared password, and rotate it if it leaks.
:::

If no code is configured by the service operator, enrollment is open and the
installer needs no flag.

## Start and connect

Public remote access uses a persistent named hostname. First-time registration
needs the [enrollment code](#enrollment-code) described above; certificate setup
is not required. Your private installation identity is saved on this machine;
later starts reuse the same hostname and do not resend the code. If the service
reports that named tunnels are unavailable or full, contact the service operator;
local TLS certificates do not fix allocation errors.

```sh
amarcoded --remote
```

To start with a project folder selected:

```sh
amarcoded --remote /path/to/project
```

Copy the Remote URL (a stable `wss://a7c42e19.amarjay.com/rpc` address) and access token. In the desktop, open **Settings → Connection**, choose **Remote daemon**, paste both, and select **Verify and apply**.

The remote desktop app does not need to be installed or open. Leave this command
running; Ctrl+C in its terminal stops the daemon and connector. Closing a client
window does not stop it. See [run without the desktop](/docs/remote/standalone/)
for foreground, service, and separate-instance lifetimes.

Starting it again reuses the same URL. Offline machines retain their allocation for a seven-day inactivity grace period.
The Worker checks daily and then reclaims disconnected tunnels to free capacity.
Preserving the local identity lets the same machine recover its hostname without
manual registration, provided capacity is available.

See [connect to your host](/docs/remote/connect/).

:::caution[Keep the token private]
In the default token-only mode, anyone with the token can use the daemon on
that machine. Keep it private even if you later add client certificates.
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

The listener is `127.0.0.1:43721`. Keep it running and follow the
[SSH port-forwarding steps](/docs/remote/ssh/#connect-the-desktop-through-ssh-port-forwarding)
to reach it from your own computer. A loopback address on your laptop is not the
remote machine. This mode does not enroll a public tunnel.

## Choose a company connection profile

The default Cloudflare tunnel needs no certificates of your own, but Cloudflare
terminates public TLS. For a company project, use the approved profile:

- [Direct TLS and client certificates](/docs/remote/certificates/)
- [Paired end-to-end encryption](/docs/remote/end-to-end/)
- [Encryption and trust boundaries](/docs/remote/security/)

Advanced security settings must be configured on the running host and the native
client as described in those guides. They are not enabled by `--remote` alone.

## Remove a named allocation

Stop the connector and run:

```sh
amarcoded tunnel --rm
```

Use the same installation/root environment as the connector you intend to remove.
This revokes the connector, removes its public DNS record, and frees installation
capacity. The old hostname is never assigned to another installation. Run this
before purging local installation data. Keep `named-installation.json` private
and preserve it across normal restarts and daemon upgrades.

Public remote access uses named tunnels exclusively. The private loopback mode
above does not require allocator enrollment.
