---
title: Permissions
description: Review file edits, commands, and permission choices in Amarcode.
---

Agents can ask Amarcode to approve an action before they perform it. The request shows the action's details and the choices supplied by that agent.

## Review an action

Check the target path, command, arguments, and working directory before choosing an approval option. A permission request and an agent question are different: questions ask for information, while permissions authorize an action.

Depending on the agent, choices can include allowing or rejecting once, or remembering the decision for the session.

## Permission modes

Amarcode's fallback permission modes are:

| Mode | Behavior |
| --- | --- |
| Confirm | Show requests for your decision. |
| Autoedit | Automatically approve requests recognized as file edits; command requests still need review. |
| Agentic | Automatically approve agent permission requests. |

When an agent advertises its own ACP session mode, Amarcode defers to that mode instead of applying its fallback automatic approvals. Available controls therefore depend on your agent.

:::caution[Automatic approval]
Automatic approval can select a session-wide allowance when the agent offers one. Use **Confirm** when you want to review each requested action.
:::

## Built-in agent permissions

Amarcode ACP asks permission for file writes, targeted edits, moves, deletion, and commands. Read and search tools do not require the same write approval.

Its remembered decisions use exact operation keys: the target path for file changes, or the executable, full argument list, and working directory for a command. They are discarded when the ACP session closes.

Commands run through the daemon's terminal lifecycle. Executables and arguments are passed separately rather than automatically interpreted by a shell. An explicitly approved shell command can still perform the operations that shell allows.

## Permissions are not a sandbox

Workspace file tools validate paths and reject symlink escapes. Approved processes run with the daemon user's operating-system permissions. Treat agent installation and command approval as granting access on that host.
