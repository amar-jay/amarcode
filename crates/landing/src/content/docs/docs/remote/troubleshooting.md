---
title: Updates and troubleshooting
description: Update a remote daemon and resolve connection, authentication, and agent installation problems.
---

## Update a remote host

Managed daemons use the same signed update lifecycle whether the desktop
connects locally, over SSH, or over WebSocket. Use the desktop's daemon update
action while connected to that host. The host's supervisor performs activation
and rollback; the desktop retries status over the remote connection while the
listener restarts. It never reads the laptop's local journal for a remote update.

The headless installer reuses an existing verified current release. Rerun it to
configure access, install runtimes, or migrate an old remote layout; it does not
silently switch a running service to another binary. Managed updates require the
standard running user service. A foreground/custom daemon can report updates as
unavailable; install the standard service on its host with
`amarcode-daemon bootstrap install` when appropriate.

Restarting the daemon interrupts active agent processes. The SSH profile keeps
its host and port. A separate `amarcode-daemon tunnel` connector stays independent
of the daemon, although a new connector run gets a new public hostname.

## Update a local installation

The desktop checks for app and daemon updates separately. Use the update action it offers for each. Daemon updates verify the signed artifact, activate the candidate, and check its exact version and protocol before committing. Failed activation attempts restore the verified previous release.

Managed daemon self-updates currently target supported Linux and Windows services with standard configuration. Custom transports, addresses, data paths, and development binaries can report updates as unavailable.

## The WebSocket connection fails

Check these in order:

1. The host’s `amarcode-daemon --remote` command is still running, or its service and separate `amarcode-daemon tunnel` helper are both running.
2. The URL is from the current run and ends in `/rpc`.
3. You entered the access token separately, without query parameters in the URL.
4. The desktop and daemon use the same protocol version.
5. The host and desktop have network access to the tunnel endpoint.

An old quick-tunnel URL cannot be reused after the hostname changes.

## SSH will not connect

First confirm you can connect to the same host or SSH config alias with your SSH
client. Then check the **Current SSH connection** status in **Settings → Connection**.
On the host, run `amarcoded ssh`. It starts SSH access or reuses the existing
authenticated listener. If it asks for a restart, run `amarcoded restart` (or
restart the existing foreground daemon) when active work can be interrupted.
It needs a valid private token file and does not require Cloudflare. Preserve
custom environment settings when restarting a deliberately isolated instance.

| Error or symptom                     | What to check                                                                                                                                                                                                                        |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| OpenSSH could not start              | Install the OpenSSH client on the desktop and make sure `ssh` is on its `PATH`.                                                                                                                                                      |
| Host key could not be verified       | Verify the host fingerprint through a trusted channel and establish trust with your SSH client. For a changed key, confirm why it changed before updating known hosts.                                                               |
| SSH authentication failed            | Check the username, identity file, and SSH agent. Unlock encrypted keys in the agent. Password and keyboard-interactive prompts are not supported.                                                                                   |
| The daemon rejected the access token | Enter the daemon's bearer token, not an SSH key or passphrase. Changing target settings requires re-entering its token.                                                                                                              |
| Timeout or connection refused        | Check both ports: the SSH port reaches the SSH server; **Daemon port** reaches `127.0.0.1` on the remote host. Confirm the daemon is running there in authenticated WebSocket mode and that the server permits local TCP forwarding. |
| Protocol mismatch                    | Update the desktop and remote daemon to compatible releases. An SSH login alone does not establish protocol compatibility.                                                                                                           |
| Local project path does not exist    | Choose an absolute directory on the remote host. The local identity-file picker and remote workspace picker refer to different machines.                                                                                             |

The desktop retries a saved SSH connection automatically, with a maximum
five-second delay between attempts. A connection attempt can take longer than
the delay when the network or host is unresponsive. Fixing SSH access or bringing
the daemon back online lets the desktop recover without replacing the saved
profile.

If the daemon is running, reconnecting restores current turns and pending
approvals. If the daemon itself restarted, its agent processes and in-memory
requests were interrupted; saved chat history is still available. See
[Connect over SSH](/docs/remote/ssh/) for the full workflow.

## Authentication is rejected

Read the token from the host's `access-token` file and re-enter it. For manually configured hosts, `AMARCODE_WS_TOKEN_FILE` must point to a valid private token file. On Unix, its permissions must exclude group and other access, such as mode `0600`.

If the desktop says authentication is not enforced, install a daemon release with remote support and configure the token on that host. The desktop intentionally rejects an unauthenticated remote daemon.

## An agent will not install

Check `data/daemon.log` inside the remote data directory. Installation failures include a bounded tail of package-command output.

Verify outbound network access and agent platform support. If the installation predates managed Bun and uv support, rerun the current installer and restart remote mode.

## The port is already in use

The shared daemon uses local TCP port `43821` and authenticated loopback
WebSocket port `43721`. If its service is running, restart it instead of starting
a second foreground process. Stop the old `amarcode-remote.service` before
migrating an earlier installation. A database ownership error means another
process owns the same instance; do not bypass its lock or select a new database
to hide that error.

## Share a useful issue report

Include your operating system, desktop and daemon versions, the exact error, and relevant log excerpts. Remove access tokens, API keys, private prompts, and sensitive project paths before posting to [GitHub issues](https://github.com/amar-jay/amarcode/issues).
