---
title: Conversations and commands
description: Reopen chats, switch agents, use slash commands, and inspect activity.
---

## Keep a conversation with the project

Each chat stores its workspace path. Select a chat from the sidebar to return to its messages and project. A new chat can use the default workspace from **Settings → Agent defaults**.

Messages and structured message parts are stored by the daemon. They remain available after you close the desktop.

## Use slash commands

Type `/` in the composer to discover commands advertised by the selected agent. Command discovery can start an agent session without submitting a user turn.

Commands are agent-specific. A command available in one agent may not exist in another, and the catalog can change during a session. Amarcode keeps a slash command at the beginning of the prompt even when it also restores historical context.

## Change session options

Agents can advertise controls such as a model or session mode. The composer displays supported configuration options. Available values and behavior are defined by the agent.

## Stop an active turn

Use the stop control to cancel the current prompt. The session can remain available for a follow-up. Stopping a turn does not delete earlier messages.

Closing the desktop is different from cancellation: the daemon and its agent processes can continue running.

## Inspect activity

The conversation view shows agent text alongside reported reasoning, tools, diffs, and terminals. The activity view can show persisted ACP traffic and run information.

Raw ACP event recording is off by default. Enable it under **Settings → Daemon data** when you need protocol diagnostics. Recording has a configurable retention period; it is separate from durable user-visible chat history. Not every high-frequency streaming notification is retained as a raw event.
