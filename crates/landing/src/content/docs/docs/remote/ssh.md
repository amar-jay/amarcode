---
title: Connect over SSH
description: Connect your desktop to an existing remote Amarcode daemon through SSH, with host verification and automatic recovery.
---

SSH keeps the daemon's listener on the remote machine's loopback address. Your
desktop creates an encrypted tunnel and uses the same authenticated daemon API
for conversations, project files, Git, and agents. You do not need a public daemon
URL or a Cloudflare tunnel for this connection.

```text
Amarcode desktop → SSH tunnel → remote loopback daemon → project and agents
```

## Before you connect

You need:

- OpenSSH available as `ssh` on your desktop.
- SSH access to the remote host using a key or your SSH agent.
- A verified host key in your SSH client's known-hosts file.
- An Amarcode daemon already running in authenticated WebSocket mode on the
  remote host's IPv4 loopback address, usually `127.0.0.1:43721`.
- That daemon's access token and a desktop release using the same protocol.

Amarcode connects to an existing daemon. It does not install, start, or update a
remote daemon over SSH. The [remote host setup guide](/docs/remote/setup/)
describes the current installer and its Cloudflare mode. An existing loopback
WebSocket daemon can also be used directly over SSH.

### Verify SSH access first

If you use an SSH config alias, you can use the same alias in Amarcode. For
example, your desktop's SSH config might contain:

```sshconfig
Host dev-box
    HostName dev.example.com
    User developer
    Port 22
    IdentityFile ~/.ssh/id_ed25519
```

Connect with your SSH client first:

```sh
ssh dev-box
```

Verify a new host's fingerprint through a trusted channel before accepting it.
Amarcode requires strict host-key checking and rejects unknown or changed keys.
If a known host's key changes, confirm the reason with the host's administrator
before updating your trusted key.

Amarcode uses non-interactive key or agent authentication. It cannot show SSH
password, passphrase, or keyboard-interactive prompts. If your key is encrypted,
unlock it in your SSH agent before connecting. SSH agent forwarding to the
remote host is disabled.

## Enter the connection details

Open **Settings → Connection**, or **Connection settings** in the startup dialog.
Choose **SSH** under **Connection type**.

| Field                   | What to enter                                                                                                                                 |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| **SSH host or alias**   | A hostname, IP address, or alias such as `dev-box`. Enter the username separately, rather than `user@host`.                                   |
| **Username**            | Optional. Leave blank to use your SSH configuration or SSH's default username.                                                                |
| **SSH port**            | Optional. Leave blank to use your SSH configuration; otherwise SSH defaults to `22`.                                                          |
| **Daemon port**         | The daemon's WebSocket port on the remote host's `127.0.0.1`, usually `43721`. This is separate from the SSH port.                            |
| **Identity file**       | Optional. Choose a key file on your desktop with **Browse…**, or enter its absolute path. Leave blank to use your SSH configuration or agent. |
| **Daemon access token** | The bearer token configured for the remote daemon. This is separate from your SSH key or passphrase.                                          |

The identity-file picker selects a local path. Amarcode does not upload the key
file to the daemon. The SSH client handles key authentication; the daemon token
travels inside the encrypted tunnel and is not passed as an SSH command argument.

Choose **Verify and apply**. Amarcode starts the tunnel, authenticates with the
daemon, and checks health and protocol compatibility before saving. A failed
verification or settings write keeps the previous saved connection intact.

Applying a successful connection reloads the desktop. Save editor drafts first.
The app does not silently fall back to your local daemon if SSH fails.

### Reuse or change a token

For an unchanged connection, leave **Daemon access token** blank to keep the
saved token. Changing a host, username, port, or identity-file setting requires
entering the target daemon's token again.

The saved token stays in native desktop settings and is not returned to the
webview. Keep it private: it authorizes the daemon's workspace and agent
operations. SSH does not remove the daemon's token requirement.

## Open a remote project

Choose a project using its absolute path on the remote host, for example:

```text
/home/developer/projects/my-app
```

Files, Git operations, agent processes, provider credentials, and conversation
history belong to that host. Clipboard attachments are uploaded to it. Workspace
defaults and chat pins use a stable SSH profile scope, so reconnecting through a
new temporary local port does not change them.

Switching between local and remote daemons does not transfer conversation
history. See [files and Git](/docs/workspace/files-and-git/) for workspace actions.

## What happens when the connection drops

The desktop monitors its SSH process and checks authenticated daemon health. If
the connection becomes unavailable, it retries automatically with a delay that
increases from 250 milliseconds to a maximum of five seconds between attempts.
The Connection panel shows the current SSH state, errors, and retry timing.

After reconnecting, Amarcode reopens the event subscription and reloads the
current session state. It restores active turns and queued approvals or input
requests. If another client finished a turn or answered a request while you were
offline, the recovered view reflects that change.

Reconnecting does not automatically resend a prompt or repeat a mutating RPC.
Losing the desktop connection does not cancel remote agent work. Quitting the
desktop closes its local SSH tunnel and leaves the remote daemon running.

:::note[Connection recovery is different from daemon recovery]
Active turns and pending requests can be recovered while the same daemon is
still running. Restarting or stopping the daemon interrupts its agent processes;
a new daemon instance cannot resume those in-memory approvals. Saved conversation
history remains available. See [conversation continuity](/docs/concepts/continuity/).
:::

## Switch connections

Save drafts, choose another connection type, and apply it. Amarcode closes the
old SSH tunnel after the replacement settings have been saved successfully.
Existing agent runs continue on their original host.

For connection errors, follow [SSH troubleshooting](/docs/remote/troubleshooting/#ssh-will-not-connect).
