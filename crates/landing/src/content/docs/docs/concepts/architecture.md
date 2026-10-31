---
title: Desktop, daemon, and agents
description: What each part of Amarcode does, and where your work is stored.
---

## Desktop

You chat, edit files, review changes, and change settings here. Save drafts before you reload the app or switch connections.

## Daemon

The daemon stores chats and runs agents and file operations. One installation serves this user on each computer. The app and the `amarcoded` command use that same installation.

Closing the app leaves the daemon running. Stopping the daemon stops active agent work. Saved chats remain.

Uninstalling the app also leaves the daemon. See [removal](/docs/getting-started/installation/#remove-the-desktop-or-daemon).

## Agents

Agents read the project, edit files, and run commands. Models and commands depend on the agent you pick.

## Where work is stored

Chats, attachments, installed agents, and keys stay on the daemon’s computer. Connecting to a remote host does not copy your local history there.
