---
title: Updates and troubleshooting
description: Update a remote daemon and resolve connection, authentication, and agent installation problems.
---

## Update Amarcode

Choose **Check for updates** in the desktop. One dialog includes the app and the connected daemon. When connected remotely, check the host shown before starting the update.

Updating the daemon interrupts active agent work and terminal sessions. Saved chats remain available. If an update fails, follow the error message and retry.

Automatic daemon updates require a managed service. For a foreground or custom installation, stop the daemon and follow your host's installation procedure.

## The connection fails

Check these in order:

1. The remote daemon and its connection helper are still running.
2. You are using the current URL, including its `/rpc` path.
3. The access token belongs to that host and was pasted into the token field.
4. The desktop and daemon are up to date.
5. Both machines have internet access.

Restarting the connection helper can change its URL. Copy the new URL into **Settings → Connection**.

## Authentication is rejected

Copy the token again from the remote host's `access-token` file. Keep it separate from the URL.

If you use a token file on Unix, make it private with `chmod 600 /path/to/token`. A token for your local daemon may differ from the remote host's token.

## Connection refused in a container

Make sure the daemon is running. Installing it or having a service file does not start a working service in containers without systemd.

Follow the [tmux setup for containers](/docs/remote/setup/#codespaces-and-containers-without-systemd). Install `tmux` first if the shell reports `command not found`.

## The port is already in use

A daemon may already be running. Use `amarcoded restart` for an existing managed service instead of starting a second process. Stop any foreground daemon before starting another one.

## Remote terminal will not connect

Run `amarcoded ssh` from an interactive terminal and use the current remote URL and token.

- **Unauthorized:** check that you used the remote host's token.
- **Not found:** update the remote daemon.
- **Timeout or connection refused:** check the daemon, connection helper, and URL.
- **Token file error:** correct the file path and permissions, or unset `AMARCODE_WS_TOKEN_FILE` to paste a token.
- **Disconnected:** reconnect and reattach your `tmux` session.

See [remote terminal](/docs/remote/ssh/).

## Installer reports a GitHub rate limit

Wait until the reset time shown in the error, then retry. On shared hosts, an exported `GH_TOKEN` or `GITHUB_TOKEN` can provide an authenticated download allowance.

If a token is rejected, refresh it or unset it before retrying.

## An agent will not install

Confirm the host has internet access and the agent supports its operating system. Retry from the agent picker. For more detail, check `data/daemon.log` in the host's Amarcode data directory.

## Report a problem

Include your operating system, app and daemon versions, the error, and relevant logs in a [GitHub issue](https://github.com/amar-jay/amarcode/issues).

Remove access tokens, API keys, private prompts, and sensitive paths before posting.
