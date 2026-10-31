---
title: Desktop, daemon, and agents
description: Understand which part of Amarcode owns your files, conversations, processes, and credentials.
---

Amarcode separates its interface from the process that owns your workspace. This lets the same desktop connect to a daemon locally or on another host.

## The desktop

The React interface and Tauri shell provide your conversation view, editor, settings, clipboard integration, and microphone access. The desktop forwards workspace and chat operations to the daemon.

Editor drafts are held in the desktop's memory. Save them before a reload or connection change.

## The daemon

The Rust daemon is the system of record. It owns:

- SQLite chats, messages, structured parts, and agent-run records.
- ACP agent subprocesses and client-owned terminals.
- Project file reads, saves, Git operations, and attachments.
- Provider credentials and agent installations on its host.
- Local service lifecycle and managed daemon update jobs.

Local clients communicate over a TCP JSON-line API. Remote desktop connections
use authenticated WebSockets through the remote daemon URL. Enabling remote
access adds a WebSocket listener
to the same process that serves local TCP. Both connections share the same RPC
methods, sessions, pending approvals, event bus, and database.

There is one managed installation per user per host: desktop bootstrap and the
headless installer share the current release, immutable binaries, stable command,
and service updater. "Remote" selects a connection to that host, rather than a
second database or binary installation. An application configured for a remote
host does not need to install a local daemon. Explicit custom instance paths
remain available for isolation; two processes cannot own the same database.

## Coding agents

Agents speak ACP over standard input and output. The daemon initializes a process, creates or resumes its session, sends prompts, and handles incoming updates and permission requests.

Agents differ in their supported commands, models, authentication methods, and session persistence. Amarcode ACP is the built-in adapter for OpenAI-compatible providers.

## Durable state comes first

The normal session write path stores product state before publishing the corresponding UI notification. The desktop can reload messages from the daemon instead of treating streamed UI state as the only copy.

SQLite uses WAL mode and foreign keys. A database instance lock prevents a second daemon from performing recovery against a database already owned by another process.

## Closing the window

Closing the desktop does not stop the managed daemon. Active agent work can continue. A foreground daemon must remain running; stopping it ends its agent processes. The optional `amarcode-daemon tunnel` connector has its own lifetime: stopping it closes public access and leaves the daemon running.

Removing the desktop application also preserves the shared daemon, its service,
and its data. Removing the daemon is a separate, explicit local action because
other clients may still use it. See [desktop and daemon removal](/docs/getting-started/installation/#remove-the-desktop-or-daemon).

## Where your data lives

The desktop stores its remote connection URL and token locally.
[Remote terminal access](/docs/remote/ssh/) uses that host's authenticated daemon
API to open an interactive shell from the CLI.

The active daemon host owns its conversations, files, attachments, and provider keys. A remote connection does not move the local daemon's history to that host.

Provider requests leave the host for the provider you configured. Amarcode's local persistence does not replace the provider's own retention policies.
