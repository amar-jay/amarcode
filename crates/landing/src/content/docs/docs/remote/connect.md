---
title: Connect from the desktop
description: Connect Amarcode to an authenticated remote daemon and choose a remote project.
---

Choose SSH for a private connection to an existing daemon, or WebSocket for a
daemon exposed through a tunnel. Projects and agents run on the selected host.

## Connect over WebSocket

First [start the shared daemon and attach a Cloudflare tunnel](/docs/remote/setup/#optional-cloudflare-access) on your Linux host, then copy the WebSocket URL and access token.

1. Open **Settings → Connection**, or **Connection settings** in the startup dialog.
2. Choose **Remote WebSocket** under **Connection type**.
3. Enter the complete `wss://host/rpc` URL.
4. Paste the host's access token into **Daemon access token**.
5. Choose **Verify and apply**.

Amarcode verifies authentication, health, and protocol compatibility before saving. A failed connection does not silently fall back to your local daemon.

Remote connections require `wss://`. Plain `ws://` is allowed only for loopback testing. Tokens are sent in an authorization header, never in the URL. The desktop stores the token in native settings and does not return the saved token to the webview.

## Connect over SSH

For prerequisites, an SSH config example, and a field-by-field walkthrough, see
[Connect over SSH](/docs/remote/ssh/).

The desktop needs an installed OpenSSH client. The remote daemon must already
be running in authenticated WebSocket mode on its IPv4 loopback address,
usually `127.0.0.1:43721`. Amarcode does not install or start it over SSH.

1. Verify the host's fingerprint using your SSH client and establish trust in its known-hosts file.
2. Open **Settings → Connection** and choose **SSH**.
3. Enter the SSH host or an alias from your SSH config.
4. Optionally enter a username, SSH port, and local identity-file path. **Browse…** selects a file on your desktop. Blank fields use SSH configuration and the SSH agent; the default SSH port is 22 when the config does not override it.
5. Enter the remote daemon's loopback port and its **Daemon access token**.
6. Choose **Verify and apply**.

SSH uses key or agent authentication. Unlock encrypted keys in your SSH agent
before connecting. Unknown or changed host keys are rejected; Amarcode never
disables host verification. Password and interactive authentication prompts are
not supported. SSH agent forwarding is disabled.

Amarcode forwards a local loopback port through SSH, authenticates with the
daemon, and checks protocol compatibility before saving. Failed verification
keeps your existing settings and connection. A blank token preserves the saved
credential only when every target setting is unchanged. Changing the target
requires entering its token.

The connection panel shows SSH status and retry timing. If the connection drops,
Amarcode reconnects automatically and restores active turns and pending
approvals. Losing the desktop connection does not cancel remote agent runs.

## Open a project on the host

Enter an absolute remote project path when choosing a workspace:

```text
/home/me/projects/my-app
```

The daemon checks that directory on its own machine. Local desktop paths do not refer to remote files.

Install agents and configure provider credentials through the app as usual. Those actions now apply to the remote host. Clipboard attachments are uploaded to it too.

## Switch back to local

Save editor drafts first. Choose **Local daemon** under **Connection type** and
select **Verify and apply**. Amarcode closes its SSH tunnel after the switch;
the remote daemon continues running.

Workspace defaults and chat pins are scoped to the selected endpoint. Local and remote daemons have separate chat stores. Switching connections does not migrate history between hosts.

## Use a permanent hostname

For a stable address, configure a named Cloudflare Tunnel with its origin pointing to `http://127.0.0.1:43721`. Keep the daemon's bearer authentication enabled.

Use `wss://your-hostname/rpc` in Amarcode. Manage the daemon and connector with your host's process supervisor for unattended use.

The desktop does not implement Cloudflare Access interactive login or Access service-token headers. If you add an Access policy, it must permit this authenticated connection to reach the origin.

See the repository's [remote daemon guide](https://github.com/amar-jay/amarcode/blob/main/docs/remote-daemon.md) for custom origin configuration and manual deployments.
