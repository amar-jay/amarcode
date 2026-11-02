import { useState } from "react";
import {
  CircleAlert,
  KeyRound,
  LoaderCircle,
  Timer,
  Unplug,
} from "lucide-react";
import type { AgentFailureKind } from "@/types";
import { useSetAtom } from "jotai";
import { agentSetupTargetAtom } from "@/state/agents";
import { notify } from "@/lib/notify";
import {
  Alert,
  AlertAction,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

type LiveChatFailureBannerProps = {
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

function failureBannerCopy(kind: AgentFailureKind | null): {
  title: string;
  Icon: typeof CircleAlert;
} {
  switch (kind) {
    case "auth_required":
      return { title: "Sign in required", Icon: KeyRound };
    case "unavailable":
      return { title: "Agent runtime unavailable", Icon: CircleAlert };
    case "adapter_exited":
      return { title: "Agent adapter stopped", Icon: Unplug };
    case "timeout":
      return { title: "Agent timed out", Icon: Timer };
    default:
      return { title: "Agent turn failed", Icon: CircleAlert };
  }
}

export function LiveChatFailureBanner({
  error,
  errorKind,
  authRequired,
  canDeleteChat,
  onSignedIn,
  onDeleteChat,
}: LiveChatFailureBannerProps) {
  const openSetup = useSetAtom(agentSetupTargetAtom);
  const [deleting, setDeleting] = useState(false);
  const { title, Icon } = failureBannerCopy(errorKind);
  const showSignIn = errorKind === "auth_required" && Boolean(authRequired);
  const busy = deleting;

  return (
    <Alert
      variant="destructive"
      className="mx-auto mb-3 max-w-3xl border-destructive/30 bg-destructive/5 px-3 py-3 has-data-[slot=alert-action]:pr-28"
      aria-live="assertive"
    >
      <Icon />
      <AlertTitle className="text-sm">{title}</AlertTitle>
      <AlertDescription className="text-sm text-destructive/90">
        {error}
      </AlertDescription>
      {(showSignIn || canDeleteChat) && (
        <AlertAction className="top-2.5 right-2.5 flex items-center gap-2">
          {showSignIn && (
            <Button
              size="sm"
              variant="outline"
              className="border-destructive/40 text-destructive hover:bg-destructive/10"
              disabled={busy}
              onClick={() => {
                if (!authRequired) return;
                openSetup({
                  id: authRequired.agentId,
                  name: authRequired.agentId,
                  onReady: onSignedIn,
                });
              }}
            >
              Sign in
            </Button>
          )}
          {canDeleteChat && (
            <Button
              size="sm"
              variant="outline"
              className="border-destructive/40 text-destructive hover:bg-destructive/10"
              disabled={busy}
              onClick={() => {
                setDeleting(true);
                void onDeleteChat()
                  .catch((cause: unknown) => {
                    notify(
                      cause instanceof Error
                        ? cause.message
                        : "Failed to delete chat",
                      "error",
                    );
                  })
                  .finally(() => setDeleting(false));
              }}
            >
              {deleting ? (
                <span className="inline-flex items-center gap-1.5">
                  <LoaderCircle className="size-3.5 animate-spin" />
                  Deleting…
                </span>
              ) : (
                "Delete chat"
              )}
            </Button>
          )}
        </AlertAction>
      )}
    </Alert>
  );
}
