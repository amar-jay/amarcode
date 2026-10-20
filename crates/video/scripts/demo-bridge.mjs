// Only injected by Playwright. The product code and its components stay untouched.
export function installDemoBridge() {
  const now = new Date().toISOString();
  localStorage.setItem("amarcode-verbose-reasoning", JSON.stringify(true));
  localStorage.setItem("amarcode-theme", JSON.stringify("light"));
  localStorage.setItem("amarcode-palette", JSON.stringify("monochrome"));
  localStorage.setItem("amarcode-default-agent", JSON.stringify("codex"));
  localStorage.setItem(
    "amarcode-default-workspace",
    JSON.stringify("/projects/orbit"),
  );
  const agents = [
    ["codex", "Codex", "openai"],
    ["claude", "Claude", "claude"],
    ["grok", "Grok", "grok"],
    ["cursor", "Cursor", null],
    ["gemini", "Gemini", null],
  ].map(([id, name, icon]) => ({
    id,
    name,
    icon: icon ? `/agents/${icon}.svg` : null,
    is_builtin: id === "amarcode-acp",
    command: id,
    arguments: [],
    environment: [],
    available: true,
    created_at: now,
    updated_at: now,
    resolved_command: `/usr/bin/${id}`,
    unavailable_reason: null,
  }));
  const chat = {
    id: "launch",
    workspace_path: "/home/manan/Desktop/code/amarcode/crates/landing",
    title: "Center the down icon",
    created_at: now,
    updated_at: now,
    archived_at: null,
  };
  const chats = [
    chat,
    ...[
      ["Read through repository", 2 * 60_000],
      ["GEPA ICLR paper review", 17 * 3_600_000],
      ["Inspect the git history", 18 * 3_600_000],
      ["Fix the Python module build", 20 * 3_600_000],
      ["Review daemon context restore", 4 * 86_400_000],
    ].map(([title, age], i) => ({
      ...chat,
      id: `chat-${i}`,
      title,
      updated_at: new Date(Date.now() - age).toISOString(),
    })),
  ];
  const part = (messageId, ordinal, kind, value) => ({
    message_id: messageId,
    ordinal,
    kind,
    content_json: JSON.stringify(value),
  });
  const commandPart = (id, command, args, output) => ({
    sessionUpdate: "tool_call",
    toolCallId: id,
    kind: "execute",
    status: "completed",
    rawInput: { command, args },
    content: [{ type: "terminal", terminalId: id }],
    rawOutput: { exit_code: 0, formatted_output: output },
  });
  let turns = [];
  let promptCount = 0;
  let created = true;
  const message = (id, role, content, agent = "codex") => ({
    message: {
      id,
      chat_id: "launch",
      agent_run_id: `run-${agent}`,
      role,
      content,
      status: "complete",
      created_at: now,
      updated_at: now,
    },
    parts: [],
    agent_id: role === "assistant" ? agent : null,
  });
  const userTurn = message(
    "u-seed",
    "user",
    "Center the down icon and remove its label.",
  );
  const assistantTurn = message(
    "a-seed",
    "assistant",
    "",
    "codex",
  );
  assistantTurn.parts = [
    part(assistantTurn.message.id, 0, "thinking", {
      text: "The arrow should sit in the center of the bottom bar, and the label next to it should go.",
    }),
    part(assistantTurn.message.id, 1, "text", {
      text: "I'll center the down icon and remove the label, keeping the current scroll behavior.",
    }),
    part(
      assistantTurn.message.id,
      2,
      "tool_call",
      commandPart(
        "git-diff",
        "git",
        ["diff", "--", "src/components/cloud-hero.tsx"],
        "src/components/cloud-hero.tsx\n@@ bottom bar\n- label beside the icon\n+ icon centered",
      ),
    ),
    part(
      assistantTurn.message.id,
      3,
      "tool_call",
      commandPart(
        "git-log",
        "git",
        ["log", "--oneline", "-n", "5"],
        "a41c2e1 Center the hero controls\n9f03ab8 Restore the landing copy\nc88e091 Keep the cloud shader\n4d21aa0 Open the first conversation",
      ),
    ),
    part(assistantTurn.message.id, 4, "tool_call", {
      toolCallId: "edit-hero",
      kind: "edit",
      status: "completed",
      title: "File changes",
      content: [
        {
          type: "diff",
          path: "src/components/cloud-hero.tsx",
          oldText: "<span>Down</span>",
          newText: "",
        },
      ],
    }),
    part(assistantTurn.message.id, 5, "text", {
      text: "The icon is centered and the label is gone. The build still passes.",
    }),
  ];
  turns = [userTurn, assistantTurn];
  const answers = [
    "Created src/CommandPalette.tsx with instant search, keyboard navigation, and ⌘K to open.\n\nThe palette is ready for its next iteration.",
    "Added a subtle fade and scale to Codex’s command palette.\n\nThe existing search and keyboard controls stay exactly as they were.",
    "Updated Claude’s animation to respect reduced motion.\n\nThe palette still opens with ⌘K, and keeps the same search and keyboard behavior.\n\n**Ready for review.**",
  ];
  const config = [
    {
      id: "approval",
      name: "Approval",
      category: "approval",
      type: "select",
      current_value: "auto",
      options: [
        { value: "auto", name: "Auto review" },
        { value: "manual", name: "Manual" },
      ],
    },
    {
      id: "mode",
      name: "Mode",
      category: "mode",
      type: "select",
      current_value: "default",
      options: [
        { value: "default", name: "Default" },
        { value: "plan", name: "Plan" },
      ],
    },
    {
      id: "model",
      name: "Model",
      category: "model",
      type: "select",
      current_value: "sol",
      options: [{ value: "sol", name: "6.1 Sol" }],
    },
    {
      id: "effort",
      name: "Effort",
      category: "effort",
      type: "select",
      current_value: "low",
      options: [
        { value: "low", name: "Low" },
        { value: "high", name: "High" },
      ],
    },
    {
      id: "fast",
      name: "Fast mode",
      category: "other",
      type: "boolean",
      current_value: false,
      options: null,
    },
  ];
  const detail = () => ({
    chat,
    messages: turns,
    session_config: config,
    session_commands: null,
    context_usage: { used: 28400, size: 200000, cost: null },
  });
  const before = `import { Command } from "cmdk";\n\nexport function CommandPalette({ open, onClose }) {\n  return (\n    <Command.Dialog open={open} onOpenChange={onClose}>\n      <Command.Input placeholder="Where to next?" />\n      <Command.List>\n        <Command.Empty>No results found.</Command.Empty>\n        <Command.Group heading="Navigate">\n          <Command.Item>Dashboard</Command.Item>\n          <Command.Item>Projects</Command.Item>\n          <Command.Item>Settings</Command.Item>\n        </Command.Group>\n      </Command.List>\n    </Command.Dialog>\n  );\n}`;
  const after = before
    .replace(
      "import { Command }",
      'import { motion, useReducedMotion } from "motion/react";\nimport { Command }',
    )
    .replace(
      "  return (",
      "  const reducedMotion = useReducedMotion();\n\n  return (",
    )
    .replace(
      "      <Command.Input",
      "      <motion.div\n        initial={{ opacity: 0, scale: 0.98 }}\n        animate={{ opacity: 1, scale: 1 }}\n        transition={{ duration: reducedMotion ? 0 : 0.18 }}\n      >\n      <Command.Input",
    )
    .replace(
      "    </Command.Dialog>",
      "      </motion.div>\n    </Command.Dialog>",
    );
  let callbackId = 1;
  const callbacks = new Map();
  const streams = [];
  const emit = (event) => streams.forEach((s) => s.onmessage(event));
  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
    transformCallback(fn) {
      const id = callbackId++;
      callbacks.set(id, fn);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    convertFileSrc: (path) => path,
    invoke: async (cmd, args = {}) => {
      if (cmd === "daemon_bootstrap") {
        args.onStatus.onmessage({ status: "ready", version: "0.1.0" });
        return {
          status: "ok",
          version: "0.1.0",
          protocol_version: 11,
          addr: "127.0.0.1:43821",
        };
      }
      if (cmd === "subscribe_events") {
        streams.push(args.onEvent);
        args.onStatus.onmessage({ status: "connected" });
        return new Promise(() => {});
      }
      if (cmd === "daemon_check_update" || cmd === "app_check_update")
        return { status: "upToDate", currentVersion: "0.1.0" };
      if (cmd === "list_agents") return agents;
      if (cmd === "list_chats")
        return created ? [chat, ...chats.slice(1)] : chats.slice(1);
      if (cmd === "create_chat" || cmd === "update_chat") {
        created = true;
        chat.title = args.title || "Build a command palette for Orbit.";
        return chat;
      }
      if (cmd === "get_chat") return detail();
      if (cmd === "get_message_parts") return [];
      if (cmd === "discover_commands")
        return {
          run_id: "run-codex",
          agent_id: args.agentId,
          revision: 1,
          commands: [],
        };
      if (cmd === "list_agent_runs_for_chat")
        return [
          {
            id: "run-codex",
            chat_id: "launch",
            agent_id: "codex",
            status: "running",
            started_at: now,
            context_usage: null,
          },
        ];
      if (cmd === "list_acp_events_for_chat") return [];
      if (cmd === "get_workspace_info")
        return {
          displayName: "orbit",
          isGitRepository: true,
          branchName: "feat/command-palette",
        };
      if (cmd === "list_workspace_files")
        return [
          "src/components/cloud-hero.tsx",
          "src/App.tsx",
          "src/theme.css",
          "src/commands.ts",
          "README.md",
          "package.json",
        ];
      if (cmd === "list_workspace_changes")
        return [
          {
            path: "src/components/cloud-hero.tsx",
            status: "M",
            staged: false,
            unstaged: true,
          },
          { path: "src/theme.css", status: "M", staged: false, unstaged: true },
        ];
      if (cmd === "read_workspace_file") return after;
      if (cmd === "get_workspace_file_diff")
        return {
          path: "src/components/cloud-hero.tsx",
          status: "M",
          before,
          after,
          comparison: "Unstaged",
        };
      if (cmd === "set_session_config_option")
        return { chat_id: "launch", options: config };
      if (cmd === "prompt") {
        const index = promptCount++;
        const agent = args.agentId || "codex";
        const runId = `run-${agent}-${index}`;
        const userId = `u${index}`;
        const assistantId = `a${index}`;
        turns.push(message(userId, "user", args.text, agent));
        const assistant = message(assistantId, "assistant", "", agent);
        assistant.message.status = "streaming";
        turns.push(assistant);
        created = true;
        const update = () =>
          emit({
            type: "messageUpdated",
            payload: {
              message_id: assistantId,
              status: assistant.message.status,
            },
          });
        emit({ type: "chatUpdated", payload: { chat_id: "launch" } });
        emit({
          type: "turnUpdated",
          payload: {
            chat_id: "launch",
            run_id: runId,
            user_message_id: userId,
            status: "started",
            stop_reason: null,
            error_message: null,
            error_kind: null,
          },
        });
        if (index > 0)
          emit({
            type: "contextRestoration",
            payload: {
              chat_id: "launch",
              run_id: runId,
              source: "Continuing with saved conversation",
            },
          });
        const answer = answers[index] || answers[2];
        if (index === 0) {
          const log = [
            "a41c2e1  Add command palette search",
            "9f03ab8  Wire keyboard navigation",
            "17bd440  Start the Orbit workspace",
            "c88e091  Set the paper and ink theme",
            "4d21aa0  Open the first conversation",
          ].join("\n");
          const updates = [
            {
              sessionUpdate: "tool_call",
              toolCallId: "git",
              title: "git log",
              kind: "execute",
              status: "in_progress",
              rawInput: { command: "git", args: ["log", "--oneline", "-n", "5"] },
              content: [{ type: "terminal", terminalId: "git-log" }],
            },
            {
              sessionUpdate: "tool_call_update",
              toolCallId: "git",
              status: "completed",
              _meta: {
                terminal_info: { terminal_id: "git-log", cwd: "/projects/orbit" },
                terminal_output: {
                  terminal_id: "git-log",
                  data: log,
                  truncated: false,
                },
                terminal_exit: { terminal_id: "git-log", exit_code: 0 },
              },
            },
          ];
          assistant.parts = updates.map((value, ordinal) => ({
            message_id: assistantId,
            ordinal,
            kind: "tool_call",
            content_json: JSON.stringify(value),
          }));
          update();
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
        await new Promise((resolve) => setTimeout(resolve, 240));
        for (let step = 1; step <= 14; step++) {
          assistant.message.content = answer.slice(
            0,
            Math.ceil((answer.length * step) / 14),
          );
          update();
          await new Promise((resolve) => setTimeout(resolve, 65));
        }
        assistant.message.status = "complete";
        update();
        emit({
          type: "turnUpdated",
          payload: {
            chat_id: "launch",
            run_id: runId,
            user_message_id: userId,
            status: "completed",
            stop_reason: "end_turn",
            error_message: null,
            error_kind: null,
          },
        });
        return {
          run_id: runId,
          chat_id: "launch",
          agent_id: agent,
          user_message_id: userId,
          acp_session_id: `session-${agent}`,
        };
      }
      if (cmd.includes("is_focused")) return true;
      if (cmd.includes("listen")) return 1;
      if (cmd.includes("is_permission_granted")) return true;
      if (cmd === "get_amarcode_acp_config")
        return {
          base_url: "https://api.openai.com/v1",
          model: "Your preferred model",
          has_api_key: true,
          configured: true,
        };
      if (cmd === "get_daemon_config")
        return { store_acp_events: false, acp_event_retention_days: 7 };
      return null;
    },
  };
}
