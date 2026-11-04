import { useEffect, useRef } from "react";
import type { AgentFailureKind } from "@/types";
import { useSetAtom } from "jotai";
import { agentSetupTargetAtom } from "@/state/agents";
import { notify, notifyToast } from "@/lib/notify";

type LiveChatFailureToastProps = {
  error: string;
  errorKind: AgentFailureKind | null;
  authRequired: {
    agentId: string;
    runId: string | null;
    methods: unknown;
  } | null;
  canDeleteChat: boolean;
  onSignedIn: () => void;
  onDeleteChat: () => Promise<void>;
};

function failureTitle(kind: AgentFailureKind | null): string {
  switch (kind) {
    case "auth_required":
      return "Sign in required";
    case "unavailable":
      return "Agent runtime unavailable";
    case "adapter_exited":
      return "Agent adapter stopped";
    case "timeout":
      return "Agent timed out";
    default:
      return "Agent turn failed";
  }
}

export function LiveChatFailureToast({
  error,
  errorKind,
  authRequired,
  canDeleteChat,
  onSignedIn,
  onDeleteChat,
}: LiveChatFailureToastProps) {
  const openSetup = useSetAtom(agentSetupTargetAtom);
  const announced = useRef<string | null>(null);
  const deleting = useRef(false);
  const callbacks = useRef({ onSignedIn, onDeleteChat });
  useEffect(() => {
    callbacks.current = { onSignedIn, onDeleteChat };
  }, [onSignedIn, onDeleteChat]);
  const agentId = authRequired?.agentId;
  const runId = authRequired?.runId;
  useEffect(() => {
    const auth = errorKind === "auth_required";
    const { onSignedIn: signedIn, onDeleteChat: deleteChat } =
      callbacks.current;
    // Match the state-layer toast id so adding an action updates the existing toast.
    const id = auth
      ? `agent-auth:${agentId ?? "unknown"}:${runId ?? ""}:${error}`
      : `agent-failure:${error.trim()}`;
    const signature = `${id}:${errorKind}:${canDeleteChat}`;
    if (announced.current === signature) return;
    announced.current = signature;
    const action =
      auth && agentId
        ? {
            label: "Sign in",
            onClick: () =>
              openSetup({
                id: agentId,
                name: agentId,
                onReady: signedIn,
              }),
          }
        : !auth && canDeleteChat
          ? {
              label: "Delete chat",
              onClick: () => {
                if (deleting.current) return;
                deleting.current = true;
                void deleteChat()
                  .catch((cause: unknown) => {
                    notify(
                      cause instanceof Error
                        ? cause.message
                        : "Failed to delete chat",
                      "error",
                    );
                  })
                  .finally(() => {
                    deleting.current = false;
                  });
              },
            }
          : undefined;
    notifyToast(failureTitle(errorKind), {
      id,
      description: error,
      duration: 12_000,
      action,
    });
  }, [errorKind, error, agentId, runId, canDeleteChat, openSetup]);
  return null;
}
