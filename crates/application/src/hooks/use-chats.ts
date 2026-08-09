import { daemonApi } from "@/api";
import type { AgentDefinition, Chat } from "@/types";
import { useCallback, useRef, useState } from "react";

export type OnSelectChatSession = (session: { chat: Chat; agent?: AgentDefinition; initialRunId: string | null } | null) => void;

export function useChats(workspacePath: string, onSelectChatSession: OnSelectChatSession) {
	const [chats, setChats] = useState<Chat[]>([]);
	const requests = useRef(new Map<string, Promise<void>>());
	const latestWorkspace = useRef(workspacePath);
	latestWorkspace.current = workspacePath;

  const loadChats = useCallback(async () => {
		// The global sidebar starts with every chat, then narrows to the selected
		// workspace once the prompt screen chooses one.
		const requestKey = workspacePath || "__all_workspaces__";
		const existing = requests.current.get(requestKey);
		if (existing) return existing;
		const request = daemonApi.listChats(workspacePath || undefined)
			.then((next) => {
				if (latestWorkspace.current === workspacePath) setChats(next);
			})
			.finally(() => requests.current.delete(requestKey));
		requests.current.set(requestKey, request);
		return request;
  }, [workspacePath]);

	const handleSelectChat = (chatId: string) => {
      const chat = chats.find((item) => item.id === chatId);
      if (chat) onSelectChatSession({ chat, initialRunId: null });
    }

	const handleNewChat = () => {
		onSelectChatSession(null);
	}

	return {
		chats, 
		handleNewChat,
		handleSelectChat,
		refresh: loadChats,
	}
}
