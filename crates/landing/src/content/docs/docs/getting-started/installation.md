---
title: Install Amarcode
description: Download Amarcode and set up its local background service.
---

Amarcode has a desktop app for your conversations and a background service, called the daemon, that manages projects and agents.

## Download the desktop app

Open the [Amarcode download page](/) or [GitHub releases](https://github.com/amar-jay/amarcode/releases). Choose a published installer for your operating system and architecture. The current desktop release workflow targets Linux and Windows; use the release assets as the source of truth for available packages.

Install and open the app.

## Install the local daemon

On first launch, the connection dialog asks you to install the background service. Choose **Install** and wait for the app to connect.

The desktop includes the daemon executable, so this step does not need a separate daemon download. Installing agents and using a hosted model can still require an internet connection.

After service installation on Linux, `amarcode-daemon` is available from a new terminal. Setup adds a stable command directory to Bash, Zsh, and Fish startup configuration. No administrator access is required for PATH setup.

On Linux, the command follows daemon updates and rollbacks. Existing installations gain it when the desktop next connects to the local service. Running `amarcode-daemon uninstall` removes the terminal command and its PATH registration along with the service.

The desktop and headless installer share the same per-user installation on a host. Enabling remote access adds an authenticated listener to that daemon and preserves its existing database.

The daemon runs as a service for your user account. Closing the desktop does not stop it. On Linux it uses the systemd user manager; on Windows it uses Task Scheduler.

:::tip[Using a remote machine?]
You can connect to a remote daemon instead of installing a local service. Follow [remote setup](/docs/remote/setup/), then enter its URL and access token in **Connection settings**.
:::

## Remove the desktop or daemon

Uninstalling the desktop application preserves the shared daemon and its data.
Its service and agent work keep running, and other local or remote clients can
continue using it. Desktop upgrades also preserve the daemon.

To separately remove the daemon **on this computer**, open **Settings → General →
Remove local daemon and data** and type `DELETE LOCAL DAEMON DATA`. This stops its
service and permanently deletes its conversations, credentials, access settings,
and downloaded daemon releases. Every client connected to that instance is
interrupted. Your desktop preferences, saved remote connections, and project
directories are preserved. A daemon on another host is never removed by this
local action, even when the desktop is connected to it.

On Windows, the native uninstaller's **Delete app data** checkbox cannot remove
the shared instance: the uninstaller preserves it and explains the separate
removal flow. Use that explicit in-app action before uninstalling if you want to
remove both the desktop and the local daemon.

The CLI also provides `amarcode-daemon uninstall` to unregister only the service,
or `amarcode-daemon purge --confirm-data-loss` to delete its data. Stop a
foreground daemon before purging its database. If the command is not on PATH,
use the managed executable's full path.

## Choose an agent

Open the agent picker and select an available agent, or install one from the catalog. Some agents need you to sign in; others use a provider API key.

For the built-in **Amarcode ACP** agent, configure **Settings → ACP provider**. See [configure a provider](/docs/agents/provider/).

## If setup stops

- **Service installation fails:** confirm your user session has a working service manager. For a headless Linux host without systemd, use [remote mode](/docs/remote/setup/).
- **The daemon needs repair:** use the installation or repair action offered by the connection dialog. It preserves chat data.
- **An incompatible protocol is reported:** update the desktop and daemon together. The desktop requires the same protocol version as its daemon.

Continue with [your first conversation](/docs/getting-started/first-conversation/).
