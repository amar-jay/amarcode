---
title: Choose and install an agent
description: Use ACP-compatible coding agents with Amarcode.
---

Amarcode communicates with coding agents through the Agent Client Protocol (ACP). Each agent brings its own model support, authentication, commands, and session behavior.

## Install from the catalog

Open the agent picker in the composer or **Settings → Agent defaults**. Available agents are listed separately from agents that need installation.

![The agent picker, including built-in Amarcode ACP and installed agents.](/images/workspace/agent-picker.webp)

Select the install action for an agent and wait for it to finish. The daemon installs and launches agents on its own host. With a remote connection, installation happens on the remote machine.

The catalog comes from Amarcode's ACP registry checkout. Package-based agents use Bun or uv tooling; availability depends on the host's installed tools and package cache. Binary distributions depend on platform support.

:::note
An agent being installed does not mean it is authenticated. Complete the agent's sign-in flow before sending a prompt.
:::

## Use the built-in agent

**Amarcode ACP** is maintained separately from the external catalog. It connects an OpenAI-compatible chat-completion provider to ACP and includes file and terminal tools.

Configure it in **Settings → ACP provider**. Amarcode installs a signed release of the adapter and stores provider credentials on the daemon host.

[Configure a provider →](/docs/agents/provider/)

## Switch agents in a conversation

Choose another agent in the composer before your next prompt. The saved conversation stays in Amarcode. If that agent cannot resume a suitable native session, it receives historical context with your next request.

Agent-specific commands, model choices, and permissions may differ. Hidden model state does not transfer between agents. Read [conversation continuity](/docs/concepts/continuity/) before relying on cross-agent restoration for a long task.

## When installation fails

Check the error message for missing runtimes, an unavailable distribution, or a failed package download. Remote installations record agent install activity in the host's `data/daemon.log` under the remote data directory.

For remote hosts installed with an older installer, rerun the current installer and restart remote mode to add the managed Bun and uv runtimes.
