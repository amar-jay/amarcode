import { daemonApi } from "@/api";
import type { AgentDefinition, Chat } from "@/types";
import { useCallback, useRef, useState } from "react";

export type OnSelectChatSession = (session: { chat: Chat; agent?: AgentDefinition; initialRunId: string | null } | null) => void;

export function useChats(onSelectChatSession: OnSelectChatSession) {
	const [chats, setChats] = useState<Chat[]>([]);
	const request = useRef<Promise<void> | null>(null);

  const loadChats = useCallback((): Promise<void> => {
		if (request.current) return request.current;
		request.current = daemonApi.listChats()
			.then(setChats)
			.finally(() => { request.current = null; });
		return request.current;
  }, []);

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
