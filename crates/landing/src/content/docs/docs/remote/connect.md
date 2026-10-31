---
title: Connect from the desktop
description: Connect Amarcode to an authenticated remote daemon and choose a remote project.
---

Connect using the remote host's URL and access token. Your projects and agents run on that host.

## Connect from the app

First [install and start the daemon](/docs/remote/setup/#start-and-connect) on your Linux host.

1. Open **Settings → Connection**, or **Connection settings** in the startup dialog.
2. Choose **Remote daemon** under **Connection type**.
3. Enter the full `wss://host/rpc` URL printed by the host.
4. Paste its access token into **Daemon access token**.
5. Choose **Verify and apply**.

Use the URL and token from the current host session. Keep the token private and separate from the URL.

## Open a remote project

Choose a workspace using an absolute path on the remote host, for example:

```text
/home/me/projects/my-app
```

Local desktop folders are not available to the remote daemon. Agents and provider settings you manage while connected also belong to that remote host.

## Open a remote terminal

Run `amarcoded ssh` in your terminal and enter the same URL and token. See [remote terminal](/docs/remote/ssh/).

## Switch back to local

Save editor drafts first. Choose **Local daemon** under **Connection type** and select **Verify and apply**.

The remote daemon keeps running. Each host keeps its own conversations; switching connections does not move history between them.

If a connection fails, see [troubleshooting](/docs/remote/troubleshooting/).
