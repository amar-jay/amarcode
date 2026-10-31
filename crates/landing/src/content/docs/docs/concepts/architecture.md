---
title: Desktop, daemon, and agents
description: Understand which part of Amarcode owns your files, conversations, processes, and credentials.
---

Amarcode has three parts: the desktop app, a background service called the daemon, and coding agents.

## The desktop

The desktop is where you chat, edit files, review changes, and manage settings. Save editor drafts before reloading the app or switching connections.

## The daemon

The daemon manages your conversations, project files, agents, and credentials. It runs on your computer for local work or on another machine for remote work.

There is one managed daemon installation per user on each host. The desktop and command-line tools use that same installation.

## Coding agents

Agents read your project, answer questions, edit files, and run commands. Amarcode connects to agents through the Agent Client Protocol (ACP). Available models and commands depend on the agent.

## Closing the window

Closing the desktop leaves the managed daemon running, so active agent work can continue. Stopping or restarting the daemon interrupts that work.

Uninstalling the desktop also leaves the daemon and its data in place. See [removal options](/docs/getting-started/installation/#remove-the-desktop-or-daemon).

## Where your data lives

Conversations, attachments, agent installations, and provider keys stay on the daemon host. Connecting to a remote host does not copy your local history there.

Prompts are sent to the selected agent's model provider. Dictation sends audio to Amarcode's transcription service. These services have their own data-handling policies.
