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

Local clients communicate over a TCP JSON-line API. Remote desktop connections use authenticated WebSockets. Both transports share the same RPC methods.

## Coding agents

Agents speak ACP over standard input and output. The daemon initializes a process, creates or resumes its session, sends prompts, and handles incoming updates and permission requests.

Agents differ in their supported commands, models, authentication methods, and session persistence. Amarcode ACP is the built-in adapter for OpenAI-compatible providers.

## Durable state comes first

The normal session write path stores product state before publishing the corresponding UI notification. The desktop can reload messages from the daemon instead of treating streamed UI state as the only copy.

SQLite uses WAL mode and foreign keys. A database instance lock prevents a second daemon from performing recovery against a database already owned by another process.

## Closing the window

Closing the desktop does not stop the managed daemon. Active agent work can continue. In foreground remote mode, the host command must remain running; stopping it ends the daemon and its agent processes.

## Where your data lives

The active daemon host owns its conversations, files, attachments, and provider keys. A remote connection does not move the local daemon's history to that host.

Provider requests leave the host for the provider you configured. Amarcode's local persistence does not replace the provider's own retention policies.
