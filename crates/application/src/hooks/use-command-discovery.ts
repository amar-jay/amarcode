import { useCallback, useEffect, useRef, useState } from "react";
import { useStore } from "jotai";
import { daemonApi } from "@/api";
import {
  daemonEventStreamStateAtom,
  subscribeDaemonEvents,
} from "@/state/daemon-events";
import type { Chat, SessionCommands } from "@/types";

/** Discover on demand; opening the menu never sends a prompt. */
export function useCommandDiscovery({
  chatId,
  workspacePath,
  agentId,
  catalog,
}: {
  chatId?: string;
  workspacePath: string;
  agentId: string;
  catalog?: SessionCommands | null;
}) {
  const store = useStore();
  const key = JSON.stringify([chatId, workspacePath, agentId]);
  const prepared = useRef(new Map<string, Chat>());
  const preparing = useRef(new Map<string, Promise<Chat>>());
  const pending = useRef(new Map<string, Promise<void>>());
  const [local, setLocal] = useState<Record<string, SessionCommands>>({});
  const [errors, setErrors] = useState<Record<string, string | null>>({});
  const [loading, setLoading] = useState<Record<string, boolean>>({});
  const streamed = local[key];
  const advertised = catalog?.agent_id === agentId ? catalog : null;
  const current =
    advertised &&
    (!streamed ||
      streamed.run_id !== advertised.run_id ||
      advertised.revision > streamed.revision)
      ? advertised
      : streamed;

  useEffect(
    () =>
      store.sub(daemonEventStreamStateAtom, () => {
        if (store.get(daemonEventStreamStateAtom).status !== "connected")
          setLocal({});
      }),
    [store],
  );

  const remember = useCallback((key: string, catalog: SessionCommands) => {
    setLocal((previous) => {
      const existing = previous[key];
      if (
        existing?.run_id === catalog.run_id &&
        existing.revision >= catalog.revision
      )
        return previous;
      return { ...previous, [key]: catalog };
    });
  }, []);

  useEffect(
    () =>
      subscribeDaemonEvents((event) => {
        const id = chatId ?? prepared.current.get(workspacePath)?.id;
        if (
          event.type === "sessionCommandsUpdated" &&
          event.payload.chat_id === id &&
          event.payload.catalog.agent_id === agentId
        ) {
          remember(key, event.payload.catalog);
        }
        if (
          event.type === "runUpdated" &&
          ["failed", "stopped", "completed"].includes(event.payload.status)
        ) {
          setLocal((previous) =>
            Object.fromEntries(
              Object.entries(previous).filter(
                ([, catalog]) => catalog.run_id !== event.payload.run_id,
              ),
            ),
          );
        }
      }),
    [chatId, workspacePath, agentId, key, remember],
  );

  function discover(retry = false) {
    if (
      current ||
      pending.current.has(key) ||
      (!retry && errors[key]) ||
      !workspacePath ||
      !agentId
    )
      return;
    setErrors((previous) => ({ ...previous, [key]: null }));
    setLoading((previous) => ({ ...previous, [key]: true }));
    const promise = (async () => {
      try {
        let id = chatId;
        if (!id) {
          let chat = prepared.current.get(workspacePath);
          if (!chat) {
            let creating = preparing.current.get(workspacePath);
            if (!creating) {
              creating = daemonApi.createChat(workspacePath);
              preparing.current.set(workspacePath, creating);
            }
            try {
              chat = await creating;
              prepared.current.set(workspacePath, chat);
            } finally {
              preparing.current.delete(workspacePath);
            }
          }
          id = chat.id;
        }
        const result = await daemonApi.discoverCommands(id, agentId);
        // Notifications may have arrived while the RPC was still pending.
        remember(key, result);
      } catch (error) {
        setErrors((previous) => ({
          ...previous,
          [key]:
            error instanceof Error
              ? error.message
              : "Unable to discover agent commands.",
        }));
      } finally {
        setLoading((previous) => ({ ...previous, [key]: false }));
        pending.current.delete(key);
      }
    })();
    pending.current.set(key, promise);
  }

  async function preparedChat() {
    await pending.current.get(key);
    return prepared.current.get(workspacePath) ?? null;
  }

  return {
    catalog: current,
    discovering: Boolean(loading[key]),
    discover: () => discover(),
    retry: () => discover(true),
    error: errors[key],
    preparedChat,
  };
}
