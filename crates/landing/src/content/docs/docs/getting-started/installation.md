---
title: Install Amarcode
description: Download the desktop app and start its background service.
---

Amarcode is a desktop app and a background service, the daemon, that keeps your chats, project files, and agents running.

## Download the app

Open the [download page](/) or [GitHub releases](https://github.com/amar-jay/amarcode/releases) and install the package for your system.

## Start the daemon

On first launch, choose **Install** in the connection dialog and wait until the app connects.

The app already includes the daemon. Closing the window leaves it running.

The built-in agent is ready. Continue with [your first conversation](/docs/getting-started/first-conversation/).

:::tip[Using another computer?]
Follow [remote setup](/docs/remote/setup/), then paste that machine’s URL and access token in **Connection settings**.
:::

## Open a folder from your terminal

```sh
amarcode .
amarcode ~/projects/my-app
amarcode "/home/me/projects/my app"
```

This opens the desktop app in that local folder, or focuses the running app and selects it. Missing folders and file paths are rejected. The daemon’s default workspace stays unchanged. If the app is connected remotely, switch to **Local daemon** in Connection settings to apply the pending local folder request.

Linux `.deb` and `.rpm` packages install the `amarcode` executable on PATH. For an AppImage, make it executable and create a launcher using its actual location:

```sh
chmod +x /absolute/path/to/amarcode.AppImage
mkdir -p ~/.local/bin
ln -s /absolute/path/to/amarcode.AppImage ~/.local/bin/amarcode
```

Make sure `~/.local/bin` is on your shell’s PATH. On Windows, run `amarcode.exe` from the installation folder, or add that folder to your user PATH to invoke it elsewhere. On macOS, the executable is inside `amarcode.app/Contents/MacOS/amarcode`.

## Remove the desktop or daemon

Uninstalling the app leaves the daemon and its data in place.

To delete the daemon on this computer, open **Settings → General → Remove local daemon and data** and type `DELETE LOCAL DAEMON DATA`. That removes local chats, credentials, and downloaded daemon files. Your project folders stay. A daemon on another machine is not removed.

## If setup stops

- The machine has no service manager. Use [remote setup](/docs/remote/setup/) and leave the daemon running in the foreground.
- The connection dialog offers a repair. Repair keeps your chats.
- The app says the daemon is incompatible. Use **Check for updates** so the app and daemon update together.
