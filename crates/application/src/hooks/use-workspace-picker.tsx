import { useCallback, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { daemonApi } from "@/api";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";

/** Local native picker, or an explicitly host-relative path for remote workspaces. */
export function useWorkspacePicker(
  onSelect: (path: string) => void,
  current = "",
) {
  const [visible, setVisible] = useState(false);
  const [path, setPath] = useState(current);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const chooseWorkspace = useCallback(async () => {
    if ((await daemonApi.getConnection()).remote) {
      setPath(current);
      setError(null);
      setVisible(true);
      return;
    }
    const result = await open({
      directory: true,
      multiple: false,
      title: "Choose a project folder",
      defaultPath: current || undefined,
    });
    if (typeof result === "string") onSelect(result);
  }, [current, onSelect]);
  const select = async () => {
    setBusy(true);
    setError(null);
    try {
      await daemonApi.listWorkspaceFiles(path.trim());
      onSelect(path.trim());
      setVisible(false);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };
  const workspaceDialog = (
    <Dialog open={visible} onOpenChange={setVisible}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Remote project folder</DialogTitle>
          <DialogDescription>
            Enter an absolute directory path on the daemon host.
          </DialogDescription>
        </DialogHeader>
        <Input
          aria-label="Remote project folder"
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="/home/me/projects/my-app"
          onKeyDown={(event) => {
            if (event.key === "Enter" && path.trim().startsWith("/") && !busy)
              void select();
          }}
        />
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <Button
          disabled={busy || !path.trim().startsWith("/")}
          onClick={() => void select()}
        >
          {busy ? "Checking folder…" : "Use folder"}
        </Button>
      </DialogContent>
    </Dialog>
  );
  return { chooseWorkspace, workspaceDialog };
}
