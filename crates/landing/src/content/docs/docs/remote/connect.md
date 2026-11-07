---
title: Connect from the desktop
description: Point Amarcode at a remote daemon and open a project there.
---

Use the remote machine’s URL and access token. Projects and agents run on that machine.

## Connect

[Start the daemon](/docs/remote/setup/#start-and-connect) on the Linux host and copy what it prints.

1. Open **Settings → Connection**.
2. Choose **Remote daemon**.
3. Paste the Remote URL the daemon printed (a short `updates.amarcode.amarjay.com/r/…` link, or the full `wss://…/rpc` URL).
4. Paste the access token.
5. Choose **Verify and apply**.

![Connection settings with Remote daemon selected, and the WebSocket URL and access token filled in.](/images/workspace/desktop-connection.png)

Use the URL and token from the current session. Keep the token private.

## Open a project

Enter an absolute path on the remote machine:

```text
/home/me/projects/my-app
```

Folders on your desktop are not available there.

## Open a shell

In your own terminal, run `amarcoded ssh` and enter the same URL and token. See [remote terminal](/docs/remote/ssh/).

To use OpenSSH or another SSH client instead, see
[standard SSH clients](/docs/remote/ssh/#use-a-standard-ssh-client).

## Use this computer again

Save editor drafts. Choose **Local daemon** and **Verify and apply**.

The remote daemon keeps running. Each machine keeps its own chats.

If the connection fails, see [troubleshooting](/docs/remote/troubleshooting/).

## Connect in a browser

Open [Amarcode remote](https://remote.amarcode.amarjay.com), paste the daemon’s Remote URL and access token, and choose **Connect**. Use an up-to-date daemon with browser access support.

Projects and agents run on the remote machine. Your token stays in the tab’s memory; reload or disconnect to clear it. You can reconnect using the same URL and token while that daemon remains available.
