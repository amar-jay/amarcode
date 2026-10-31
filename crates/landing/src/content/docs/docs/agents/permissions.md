---
title: Permissions
description: Review what an agent asks to do before you allow it.
---

An agent can ask before it edits files or runs a command. The request shows the action and the choices that agent offers.

## Review a request

Read the path, command, and working directory. A question asks you for information. A permission request asks you to allow an action.

You can allow or reject once, or remember the choice for the session, when the agent offers those options.

## Modes

| Mode | What happens |
| --- | --- |
| Confirm | You decide each request. |
| Autoedit | File edits can be approved for you. Commands still need review. |
| Agentic | Agent requests can be approved for you. |

If the agent has its own session mode, that mode is used instead.

:::caution[Automatic approval]
Automatic approval can allow actions for the whole session. Use **Confirm** when you want to review each one.
:::

Approved commands run as the user that runs the daemon. Allowing an agent to install tools or run commands gives it access on that computer.
