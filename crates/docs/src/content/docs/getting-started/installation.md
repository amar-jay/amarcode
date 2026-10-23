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

The daemon runs as a service for your user account. Closing the desktop does not stop it. On Linux it uses the systemd user manager; on Windows it uses Task Scheduler.

:::tip[Using a remote machine?]
You can connect to a remote daemon instead of installing a local service. Follow [remote setup](/docs/remote/setup/), then enter its URL and access token in **Connection settings**.
:::

## Choose an agent

Open the agent picker and select an available agent, or install one from the catalog. Some agents need you to sign in; others use a provider API key.

For the built-in **Amarcode ACP** agent, configure **Settings → ACP provider**. See [configure a provider](/docs/agents/provider/).

## If setup stops

- **Service installation fails:** confirm your user session has a working service manager. For a headless Linux host without systemd, use [remote mode](/docs/remote/setup/).
- **The daemon needs repair:** use the installation or repair action offered by the connection dialog. It preserves chat data.
- **An incompatible protocol is reported:** update the desktop and daemon together. The desktop requires the same protocol version as its daemon.

Continue with [your first conversation](/docs/getting-started/first-conversation/).
