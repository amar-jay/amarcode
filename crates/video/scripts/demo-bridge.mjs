// Only injected by Playwright via context.addInitScript. The product code and
// its components stay untouched. Because addInitScript serializes the function
// (not its closure), this must be a fully self-contained function with all data
// and the invoke handler defined inline.
export function installDemoBridge() {
  const now = new Date().toISOString();
  const ago = (ms) => new Date(Date.now() - ms).toISOString();

  const workspace = "/home/manan/Desktop/code/amarcode/crates/landing";
  const run1 = "run-codex-1";
  const run2 = "run-claude-1";
  const chatId = "launch";
  const userMsg1 = "msg-user-1";
  const userMsg2 = "msg-user-2";

  const agents = [
    ["codex", "Codex", "openai", true],
    ["claude", "Claude", "claude", true],
    ["grok", "Grok", "grok", true],
    ["cursor", "Cursor", null, true],
    ["gemini", "Gemini", null, true],
  ].map(([id, name, icon, available]) => ({
    id,
    name,
    icon: icon ? `/agents/${icon}.svg` : null,
    is_builtin: id === "amarcode-acp",
    command: id,
    arguments: [],
    environment: [],
    available,
    created_at: now,
    updated_at: now,
    resolved_command: `/usr/bin/${id}`,
    unavailable_reason: null,
  }));

  const chats = [
    {
      id: chatId,
      workspace_path: workspace,
      title: "Center the down icon",
      created_at: ago(4 * 3600_000),
      updated_at: now,
      archived_at: null,
    },
  ].concat(
    ["Read through repository", "GEPA ICLR paper review", "Inspect the git history", "Fix the Python module build", "Review daemon context restore"].map(
      (title, i) => ({
        id: `chat-${i}`,
        workspace_path: workspace,
        title,
        created_at: ago((i + 2) * 3600_000),
        updated_at: ago((i + 1) * 3600_000),
        archived_at: null,
      }),
    ),
  );

  const textPart = (messageId, ordinal, text) => ({
    message_id: messageId,
    ordinal,
    kind: "text",
    content_json: JSON.stringify({ text }),
  });
  const thinkingPart = (messageId, ordinal, text) => ({
    message_id: messageId,
    ordinal,
    kind: "thinking",
    content_json: JSON.stringify({ text }),
  });
  const toolPart = (messageId, ordinal, value) => ({
    message_id: messageId,
    ordinal,
    kind: "tool_call",
    content_json: JSON.stringify(value),
  });

  function codexParts(messageId) {
    return [
      thinkingPart(
        messageId,
        0,
        "The down icon sits on its own row. To center it I should make it a flex child with place-items center, then verify nothing else in the header shifts.",
      ),
      toolPart(messageId, 1, {
        sessionUpdate: "tool_call",
        toolCallId: "codex-edit-1",
        kind: "edit",
        title: "Edit cloud-hero.tsx",
        name: "edit",
        rawInput: {
          command: "edit",
          args: ["cloud-hero.tsx"],
          cwd: "/home/manan/Desktop/code/amarcode/crates/landing",
        },
        content: [
          {
            type: "diff",
            path: "src/components/cloud-hero.tsx",
            changes: [
              { path: "src/components/cloud-hero.tsx", operation: "modify" },
            ],
            oldText: 'className="flex items-center justify-between gap-5',
            newText: 'className="flex items-center justify-center gap-5',
          },
        ],
      }),
      textPart(
        messageId,
        2,
        "I centered the down icon by switching its container to a flex row with `justify-center`. The header still keeps its side labels on larger screens, so nothing else moved.",
      ),
    ];
  }

  function claudeParts(messageId) {
    return [
      thinkingPart(
        messageId,
        0,
        "Codex already centered the icon. The user wants a subtle entrance, so I'll add a gentle fade and a small upward drift to the hero, respecting reduced motion.",
      ),
      toolPart(messageId, 1, {
        sessionUpdate: "tool_call",
        toolCallId: "claude-edit-1",
        kind: "edit",
        title: "Edit cloud-hero.tsx",
        name: "edit",
        rawInput: {
          command: "edit",
          args: ["cloud-hero.tsx"],
          cwd: "/home/manan/Desktop/code/amarcode/crates/landing",
        },
        content: [
          {
            type: "diff",
            path: "src/components/cloud-hero.tsx",
            changes: [
              { path: "src/components/cloud-hero.tsx", operation: "modify" },
            ],
            oldText: '<div className="relative max-w-[970px] px-7',
            newText: '<div className="relative max-w-[970px] animate-[fade-up_0.6s_ease] px-7',
          },
        ],
      }),
      textPart(
        messageId,
        2,
        "Added a subtle entrance to the hero: a quick fade with a gentle upward drift, gated behind `prefers-reduced-motion` so it stays calm.",
      ),
    ];
  }

  function message(role, id, content, parts, runId, status = "complete") {
    return {
      message: {
        id,
        chat_id: chatId,
        agent_run_id: runId,
        role,
        content,
        status,
        created_at: now,
        updated_at: now,
      },
      parts,
      agent_id: runId === run1 ? "codex" : runId === run2 ? "claude" : null,
    };
  }

  let messages = [
    message("user", userMsg1, "Center the down icon", [
      textPart(userMsg1, 0, "Center the down icon"),
    ], run1),
    message(
      "assistant",
      "msg-a-codex-1",
      "I centered the down icon by switching its container to a flex row with justify-center.",
      codexParts("msg-a-codex-1"),
      run1,
    ),
    message("user", userMsg2, "Add a subtle entrance.", [
      textPart(userMsg2, 0, "Add a subtle entrance."),
    ], run2),
    message(
      "assistant",
      "msg-a-claude-1",
      "Added a subtle entrance to the hero.",
      claudeParts("msg-a-claude-1"),
      run2,
    ),
  ];

  function chatDetail() {
    return {
      chat: chats[0],
      messages,
      session_config: [
        {
          id: "permission-mode",
          name: "Permission mode",
          type: "string",
          current_value: "confirm",
          options: [
            { value: "confirm", name: "Confirm each action" },
            { value: "auto", name: "Approve automatically" },
          ],
        },
        {
          id: "session-mode",
          name: "Session mode",
          type: "string",
          current_value: "acp",
          options: [
            { value: "acp", name: "Agent session" },
            { value: "plain", name: "New context each turn" },
          ],
        },
      ],
      context_usage: { used: 53_000, size: 200_000, cost: null },
    };
  }

  const workspaceFiles = [
    "index.html",
    "package.json",
    "tsconfig.json",
    "vite.config.ts",
    "src/App.tsx",
    "src/index.css",
    "src/main.tsx",
    "src/components/cloud-hero.tsx",
    "src/components/navigation.tsx",
    "src/components/sections.tsx",
    "src/components/agent-logos.tsx",
    "src/components/headless-daemon.tsx",
    "src/components/app-preview.tsx",
    "src/components/ui/button.tsx",
    "src/components/ui/cloud-shader.tsx",
    "src/hooks/use-download.ts",
    "src/hooks/use-operating-system.ts",
    "src/lib/agents.json",
    "src/lib/releases.ts",
    "src/lib/utils.ts",
  ];

  const changes = [
    { path: "src/components/cloud-hero.tsx", status: "M", staged: false, unstaged: true },
    { path: "src/components/navigation.tsx", status: "M", staged: true, unstaged: true },
    { path: "src/components/sections.tsx", status: "A", staged: false, unstaged: true },
  ];

  const diffContent = {
    "src/components/cloud-hero.tsx": {
      before: '<div className="flex items-center justify-between gap-5 border-t border-[#bdcdc4] bg-[#edf2ed] px-10 py-4.25">',
      after: '<div className="flex items-center justify-center gap-5 border-t border-[#bdcdc4] bg-[#edf2ed] px-10 py-4.25">',
    },
  };

  const fileContent = {
    "src/components/cloud-hero.tsx": `import { ArrowDown } from "lucide-react";
import { CloudShader } from "@/components/ui/cloud-shader";
import DownloadButton from "@/components/download-button";
import { Button } from "@/components/ui/button";

export default function CloudHero() {
  return (
    <section className="grid min-h-[calc(100svh-var(--navigation-height))] ...">
      <CloudShader>
        <div className="relative max-w-[970px] px-7 py-[clamp(20px,5svh,62px)] text-center">
          <h1>Different agents. One continuous thought.</h1>
        </div>
      </CloudShader>
      <div className="flex items-center justify-center gap-5 border-t border-[#bdcdc4] bg-[#edf2ed] px-10 py-4.25">
        <span>Amarcode / Desktop workspace</span>
        <a href="#app-preview" aria-label="Scroll to workspace preview">
          <ArrowDown size={16} aria-hidden="true" />
        </a>
      </div>
    </section>
  );
}
`,
  };

  const eventSubscribers = [];
  function emit(event) {
    for (const subscriber of [...eventSubscribers]) {
      try {
        subscriber.onmessage(event);
      } catch {}
    }
  }

  const commands = {
    daemon_health: () => ({
      status: "ok",
      version: "0.1.0",
      protocol_version: 11,
      addr: "127.0.0.1:43821",
    }),
    daemon_version: () => ({ version: "0.1.0", protocol_version: 11 }),
    daemon_bootstrap: (args) => {
      args.onStatus.onmessage({ status: "ready", version: "0.1.0" });
      return commands.daemon_health();
    },
    daemon_check_update: () => ({ status: "upToDate", currentVersion: "0.1.0" }),
    app_check_update: () => ({ status: "upToDate", currentVersion: "0.1.0" }),
    list_agents: () => agents,
    list_chats: () => chats,
    get_chat: () => chatDetail(),
    get_message_parts: (args) => {
      const ids = new Set(args.message_ids);
      return messages.flatMap((m) => (ids.has(m.message.id) ? m.parts : []));
    },
    list_workspace_files: () => workspaceFiles,
    get_workspace_info: () => ({
      displayName: "landing",
      isGitRepository: true,
      branchName: "main",
    }),
    list_workspace_changes: () => changes,
    get_workspace_file_diff: (args) => ({
      path: args.relativePath,
      status: "M",
      before: diffContent[args.relativePath]?.before ?? "",
      after: diffContent[args.relativePath]?.after ?? "",
      comparison: args.staged ? "Staged" : "Unstaged",
    }),
    read_workspace_file: (args) => fileContent[args.relativePath] ?? "",
    save_workspace_file: () => null,
    create_chat: () => chats[0],
    update_chat: (args) => {
      const chat = chats[0];
      chat.title = args.params?.title ?? chat.title;
      emit({ type: "chatUpdated", payload: { chat_id: chat.id } });
      return chat;
    },
    discover_commands: (args) => ({
      run_id: args.chat_id === chatId ? run2 : run1,
      agent_id: args.agent_id,
      revision: 1,
      commands: [
        { name: "plan", description: "Break a task into steps", input: { hint: "task" } },
        { name: "implement", description: "Make the change", input: { hint: "task" } },
        { name: "test", description: "Run the tests", input: { hint: "task" } },
      ],
    }),
    prompt: (args) => {
      const { chat_id, agent_id, text } = args;
      const isClaude = agent_id === "claude";
      const runId = isClaude ? run2 : run1;
      const userMsgId = `msg-user-live-${Date.now()}`;
      messages.push(
        message("user", userMsgId, text, [textPart(userMsgId, 0, text)], runId),
      );
      emit({ type: "chatUpdated", payload: { chat_id } });
      emit({
        type: "turnUpdated",
        payload: {
          chat_id,
          run_id: runId,
          user_message_id: userMsgId,
          status: "started",
          stop_reason: null,
          error_message: null,
          error_kind: null,
        },
      });
      emit({
        type: "runUpdated",
        payload: {
          run_id: runId,
          status: "running",
          error_message: null,
          error_kind: null,
        },
      });

      const assistantId = `msg-a-live-${Date.now()}`;
      const parts = isClaude ? claudeParts(assistantId) : codexParts(assistantId);
      const answer = parts
        .filter((p) => p.kind === "text")
        .map((p) => JSON.parse(p.content_json).text)
        .join(" ");

      messages.push(
        message("assistant", assistantId, "", [], runId, "streaming"),
      );
      const stream = [
        { kind: "thinking", at: 300, status: "streaming" },
        { kind: "tool_call", at: 900, status: "streaming" },
        { kind: "text", at: 1500, status: "streaming" },
        { kind: "complete", at: 2400, status: "complete" },
      ];
      let emitted = 0;
      const push = () => {
        const step = stream[emitted];
        if (!step) return;
        const msg = messages.find((m) => m.message.id === assistantId);
        msg.parts = parts.filter((p) => p.ordinal <= emitted);
        msg.message.content = msg.parts
          .filter((p) => p.kind === "text")
          .map((p) => JSON.parse(p.content_json).text)
          .join(" ");
        msg.message.status = step.status;
        emit({
          type: "messageUpdated",
          payload: { message_id: assistantId, status: step.status },
        });
        emitted += 1;
        setTimeout(push, step.at);
      };
      setTimeout(push, 150);

      setTimeout(() => {
        const msg = messages.find((m) => m.message.id === assistantId);
        msg.parts = parts;
        msg.message.content = answer;
        msg.message.status = "complete";
        emit({
          type: "messageUpdated",
          payload: { message_id: assistantId, status: "complete" },
        });
        emit({
          type: "sessionCommandsUpdated",
          payload: {
            chat_id,
            catalog: {
              run_id: runId,
              agent_id,
              revision: 1,
              commands: [
                { name: "plan", description: "Break a task into steps", input: { hint: "task" } },
                { name: "implement", description: "Make the change", input: { hint: "task" } },
              ],
            },
          },
        });
        emit({
          type: "turnUpdated",
          payload: {
            chat_id,
            run_id: runId,
            user_message_id: userMsgId,
            status: "completed",
            stop_reason: "end_turn",
            error_message: null,
            error_kind: null,
          },
        });
        emit({
          type: "runUpdated",
          payload: {
            run_id: runId,
            status: "completed",
            error_message: null,
            error_kind: null,
          },
        });
      }, 2600);

      return {
        run_id: runId,
        chat_id,
        agent_id,
        user_message_id: userMsgId,
        acp_session_id: `session-${runId}`,
      };
    },
    cancel: () => ({ cancelled: true, chat_id: chatId }),
    subscribe_events: (args) => {
      args.onStatus.onmessage({ status: "connected" });
      eventSubscribers.push(args.onEvent);
      return new Promise(() => {});
    },
    get_attachment: () => ({ media_type: "text/plain", data: "" }),
    get_daemon_config: () => ({
      store_acp_events: true,
      acp_event_retention_days: 30,
    }),
    get_amarcode_acp_config: () => ({
      base_url: "https://api.openai.com/v1",
      model: "gpt-5",
      has_api_key: true,
      configured: true,
    }),
  };

  localStorage.setItem("amarcode-verbose-reasoning", JSON.stringify(true));
  localStorage.setItem("amarcode-theme", JSON.stringify("light"));
  localStorage.setItem("amarcode-palette", JSON.stringify("monochrome"));
  localStorage.setItem("amarcode-default-agent", JSON.stringify("codex"));
  localStorage.setItem("amarcode-default-workspace", JSON.stringify(workspace));

  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args = {}) => {
      const handler = commands[cmd];
      if (!handler) {
        console.warn(`[demo-bridge] unhandled invoke: ${cmd}`);
        return Promise.resolve(null);
      }
      try {
        return Promise.resolve(handler(args));
      } catch (error) {
        return Promise.reject(String(error));
      }
    },
    transformCallback: () => 0,
  };
}
