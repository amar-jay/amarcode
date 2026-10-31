---
title: Connect from the desktop
description: Connect Amarcode to an authenticated remote daemon and choose a remote project.
---

Connect using the remote daemon’s URL and access token. Projects and agents
run on the selected host.

## Connect over WebSocket

First [install and start the daemon](/docs/remote/setup/#start-and-connect) on your Linux host, then copy the connection URL and access token it prints. Remote access is set up automatically.

1. Open **Settings → Connection**, or **Connection settings** in the startup dialog.
2. Choose **Remote daemon** under **Connection type**.
3. Enter the complete `wss://host/rpc` URL.
4. Paste the host's access token into **Daemon access token**.
5. Choose **Verify and apply**.

Amarcode verifies authentication, health, and protocol compatibility before saving. A failed connection does not silently fall back to your local daemon.

Remote connections require `wss://`. Plain `ws://` is allowed only for loopback testing. Tokens are sent in an authorization header, never in the URL. The desktop stores the token in native settings and does not return the saved token to the webview.

## Open a remote terminal

Run `amarcoded ssh` in your terminal and enter the remote daemon URL and access
token. This opens a shell on that host independently of the app’s connection
settings. See [Remote terminal](/docs/remote/ssh/) for token files and session
lifetime.

## Open a project on the host

Enter an absolute remote project path when choosing a workspace:

```text
/home/me/projects/my-app
```

The daemon checks that directory on its own machine. Local desktop paths do not refer to remote files.

Install agents and configure provider credentials through the app as usual. Those actions now apply to the remote host. Clipboard attachments are uploaded to it too.

## Switch back to local

Save editor drafts first. Choose **Local daemon** under **Connection type** and
select **Verify and apply**. The remote daemon continues running.

Workspace defaults and chat pins are scoped to the selected endpoint. Local and remote daemons have separate chat stores. Switching connections does not migrate history between hosts.

## Use a permanent hostname

For a stable address, configure a named Cloudflare Tunnel with its origin pointing to `http://127.0.0.1:43721`. Keep the daemon's bearer authentication enabled.

Use `wss://your-hostname/rpc` in Amarcode. Manage the daemon and connector with your host's process supervisor for unattended use.

The desktop does not implement Cloudflare Access interactive login or Access service-token headers. If you add an Access policy, it must permit this authenticated connection to reach the origin.

See the repository's [remote daemon guide](https://github.com/amar-jay/amarcode/blob/main/docs/remote-daemon.md) for custom origin configuration and manual deployments.
