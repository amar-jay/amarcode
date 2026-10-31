---
title: Choose and install an agent
description: Use ACP-compatible coding agents with Amarcode.
---

Choose a coding agent from the picker in the composer. Each agent has its own models, commands, and sign-in requirements.

![The agent picker, including built-in Amarcode ACP and installed agents.](/images/workspace/agent-picker.webp)

## Use the built-in agent

**Amarcode ACP** installs automatically and works with Amarcode Free. No sign-in or API key is needed. If its download fails, choose **Install** to retry.

To use your own model provider, open **Settings → ACP provider**. See [configure a provider](/docs/agents/provider/).

## Install another agent

Open the agent picker or **Settings → Agent defaults**, find an agent, and choose **Install**. Complete its sign-in or setup if prompted.

Agents run on the computer hosting your daemon. With a remote connection, they install and run on the remote machine.

## Switch agents

Choose another agent before sending your next prompt. Your conversation stays available, but commands, model choices, and permissions may change.

For long tasks, include a short handoff with the goal and remaining work. See [conversation continuity](/docs/concepts/continuity/).

## If installation fails

Check the error for missing tools, an unsupported platform, or a failed download. Confirm the daemon host has internet access, then retry.
