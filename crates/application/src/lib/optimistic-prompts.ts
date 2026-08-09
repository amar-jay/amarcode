import type { AgentEvent } from "@/types";

const storageKey = "amarcode:optimistic-prompts";

export type CachedPrompt = {
  id: string;
  sessionId: string;
  text: string;
  afterAgentMessageCount: number;
};

function read() {
  try {
    return JSON.parse(
      localStorage.getItem(storageKey) ?? "[]",
    ) as CachedPrompt[];
  } catch {
    return [] as CachedPrompt[];
  }
}

function write(prompts: CachedPrompt[]) {
  localStorage.setItem(storageKey, JSON.stringify(prompts));
}

export function cacheOptimisticPrompt(prompt: CachedPrompt) {
  write([...read(), prompt]);
}

export function acknowledgeOptimisticPrompt(sessionId: string, text: string) {
  const prompts = read();
  const index = prompts.findIndex(
    (prompt) => prompt.sessionId === sessionId && prompt.text === text,
  );
  if (index < 0) return undefined;

  const [acknowledged] = prompts.splice(index, 1);
  write(prompts);
  return acknowledged;
}

export function restoreOptimisticPrompts(
  sessionId: string,
  persistedEvents: AgentEvent[],
) {
  const serverUserTexts = new Set(
    persistedEvents.flatMap((event) =>
      event.kind === "message" && event.data.role === "user"
        ? [event.data.text]
        : [],
    ),
  );
  const pending = read()
    .filter(
      (prompt) =>
        prompt.sessionId === sessionId && !serverUserTexts.has(prompt.text),
    )
    .sort(
      (left, right) =>
        left.afterAgentMessageCount - right.afterAgentMessageCount,
    );
  if (!pending.length) return persistedEvents;

  const restored: AgentEvent[] = [];
  let agentMessageCount = 0;
  let promptIndex = 0;
  for (const event of persistedEvents) {
    while (
      pending[promptIndex] &&
      pending[promptIndex].afterAgentMessageCount <= agentMessageCount
    ) {
      restored.push({
        kind: "message",
        data: { sessionId, role: "user", text: pending[promptIndex].text },
      });
      promptIndex += 1;
    }
    restored.push(event);
    if (event.kind === "message" && event.data.role !== "user") {
      agentMessageCount += 1;
    }
  }
  while (pending[promptIndex]) {
    restored.push({
      kind: "message",
      data: { sessionId, role: "user", text: pending[promptIndex].text },
    });
    promptIndex += 1;
  }
  return restored;
}
