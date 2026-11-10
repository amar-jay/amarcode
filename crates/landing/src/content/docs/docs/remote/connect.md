---
title: Connect to your host
description: Point Amarcode at a remote daemon and open a project there.
---

Use the remote machine’s URL and access token. Projects and agents run on that machine.

## Connect

[Start the daemon](/docs/remote/setup/#start-and-connect) on the Linux host and copy what it prints.

1. Open **Settings → Connection**.
2. Choose **Remote daemon**.
3. Paste the full Remote URL printed by the daemon, such as `wss://your-remote-host.example/rpc`. Retired short links are not supported.
4. Paste the access token.
5. Choose **Verify and apply**.

![Connection settings with Remote daemon selected, and the WebSocket URL and access token filled in.](/images/workspace/desktop-connection.png)

Use the URL and token from that host and instance. A normal restart reuses its
saved identity; a different root or reset installation may use different values.
Keep the token private.

If your administrator supplied a certificate or E2EE profile, configure it and
[restart the native desktop from that environment](/docs/remote/certificates/#launch-the-desktop-from-that-shell)
before verifying the connection. See [paired E2EE](/docs/remote/end-to-end/) for
the required files and expected identity name.

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

Paired E2EE is not supported in the browser. Do not paste a paired native
profile's token or private keys into the hosted site. An encryption-required
daemon rejects browser access. Native certificate environment variables do not
configure a browser; direct TLS/mTLS may require IT-managed browser trust and
client-certificate setup. See [encryption and trust](/docs/remote/security/).

For the ordinary browser-enabled connection mode, open [Amarcode remote](https://remote.amarcode.amarjay.com), paste the daemon’s Remote URL and access token, and choose **Connect**. Use an up-to-date daemon with browser access support.

Projects and agents run on the remote machine. Your token stays in the tab’s memory; reload or disconnect to clear it. You can reconnect using the same URL and token while that daemon remains available.
