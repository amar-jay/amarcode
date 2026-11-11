---
title: Get started with a remote daemon
description: Install Amarcode on another computer and connect to its projects.
---

Run the daemon on the Linux computer that has your project, then connect from
Amarcode on your own computer. Files, agents, and chats stay on the remote host.
The desktop app does not need to be installed there.

## Install on the remote host

You need Bash, Python 3, `curl`, OpenSSL with Ed25519 support, and internet access.
Install Git for project Git operations and the agent catalog.

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash
```

If your tunnel service requires an enrollment code, ask its operator for one and
pass it to the installer:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash -s -- --enrollment-code '<code>'
```

The enrollment code is only needed for a new installation. It is different from
the access token you will use to connect.

## Start the daemon

On the remote host:

```sh
amarcoded --remote /path/to/project
```

Replace `/path/to/project` with an existing folder, or omit it and choose a
project after connecting. If `amarcoded` is not found, use the full start command
printed by the installer or open a new terminal. `amarcode-daemon` is the same
executable.

Leave this command running. It prints a **Remote URL** and **access token**.

## Connect from your computer

1. Open Amarcode and go to **Settings → Connection**.
2. Choose **Remote daemon**.
3. Paste the Remote URL and access token printed on the host.
4. Select **Verify and apply**.

Choose a project using its path on the remote host, such as
`/home/me/projects/my-app`. Your laptop's folders are not available there.

You can also use [Amarcode remote](https://remote.amarcode.amarjay.com) in a
browser with an up-to-date, browser-enabled daemon. Enter the same URL and token.
Company certificate or paired-encryption profiles may require the native app;
follow your administrator's instructions instead.

:::caution[Keep the token private]
Anyone with the token can access the daemon in the default connection mode.
The default tunnel needs no certificate setup, but Cloudflare can read the
traffic it relays. For sensitive company projects, use your approved connection
profile; see [encryption and trust](/docs/remote/security/).
:::

## Keep working

Closing the desktop leaves the remote daemon and its agent work running.
**Ctrl+C in the host's daemon terminal stops it.** Closing that terminal can
also stop it. Follow [Keep it running](/docs/remote/standalone/) to survive a
disconnected terminal or use a service.

To return to your own computer's projects, save any editor drafts, select
**Local daemon** in connection settings, and choose **Verify and apply**.

If something fails, see [Troubleshooting](/docs/remote/troubleshooting/).
For SSH, private networks, and company security settings, expand **Advanced** in
the sidebar. Those options are not required for the default setup.
