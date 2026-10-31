---
title: Connect from the desktop
description: Point Amarcode at a remote daemon and open a project there.
---

Use the remote machine’s URL and access token. Projects and agents run on that machine.

## Connect

[Start the daemon](/docs/remote/setup/#start-and-connect) on the Linux host and copy what it prints.

1. Open **Settings → Connection**.
2. Choose **Remote daemon**.
3. Paste the `wss://` URL, including `/rpc`.
4. Paste the access token.
5. Choose **Verify and apply**.

Use the URL and token from the current session. Keep the token private.

## Open a project

Enter an absolute path on the remote machine:

```text
/home/me/projects/my-app
```

Folders on your desktop are not available there.

## Open a shell

In your own terminal, run `amarcoded ssh` and enter the same URL and token. See [remote terminal](/docs/remote/ssh/).

## Use this computer again

Save editor drafts. Choose **Local daemon** and **Verify and apply**.

The remote daemon keeps running. Each machine keeps its own chats.

If the connection fails, see [troubleshooting](/docs/remote/troubleshooting/).
