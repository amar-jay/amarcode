---
title: Connect from the desktop
description: Connect Amarcode to an authenticated remote daemon and choose a remote project.
---

First [start remote mode](/docs/remote/setup/) on your Linux host and copy its WebSocket URL and access token.

## Save the connection

1. Open **Settings → Connection**, or **Connection settings** in the startup dialog.
2. Enable **Connect to a remote daemon**.
3. Enter the complete `wss://host/rpc` URL.
4. Paste the host's access token into **Access token**.
5. Choose **Apply and reload**.

Amarcode verifies authentication, health, and protocol compatibility before saving. A failed connection does not silently fall back to your local daemon.

![Connection settings with remote daemon enabled, a WebSocket URL, an access token, and Apply and reload.](/images/workspace/remote-connection.webp)

Remote connections require `wss://`. Plain `ws://` is allowed only for loopback testing. Tokens are sent in an authorization header, never in the URL. The desktop stores the token in native settings and does not return the saved token to the webview.

## Open a project on the host

Enter an absolute remote project path when choosing a workspace:

```text
/home/me/projects/my-app
```

The daemon checks that directory on its own machine. Local desktop paths do not refer to remote files.

Install agents and configure provider credentials through the app as usual. Those actions now apply to the remote host. Clipboard attachments are uploaded to it too.

## Switch back to local

Save editor drafts first. Disable the remote connection option and choose **Apply and reload**.

Workspace defaults and chat pins are scoped to the selected endpoint. Local and remote daemons have separate chat stores. Switching connections does not migrate history between hosts.

## Use a permanent hostname

For a stable address, configure a named Cloudflare Tunnel with its origin pointing to `http://127.0.0.1:43721`. Keep the daemon's bearer authentication enabled.

Use `wss://your-hostname/rpc` in Amarcode. Manage the daemon and connector with your host's process supervisor for unattended use.

The desktop does not implement Cloudflare Access interactive login or Access service-token headers. If you add an Access policy, it must permit this authenticated connection to reach the origin.

See the repository's [remote daemon guide](https://github.com/amar-jay/amarcode/blob/main/docs/remote-daemon.md) for custom origin configuration and manual deployments.
