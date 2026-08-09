import { useCallback, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

export function useWorkspacePicker() {
  const [workspace, setWorkspace] = useState("");

  const chooseWorkspace = useCallback(async () => {
    const path = await open({
      directory: true,
      multiple: false,
      title: "Choose a project folder",
    });
    if (typeof path === "string") setWorkspace(path);
  }, []);

  return { chooseWorkspace, setWorkspace, workspace };
}
