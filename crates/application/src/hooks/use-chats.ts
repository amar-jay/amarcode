/**
 * @deprecated Prefer `chatsAtom` / `refreshChatsAtom` / navigation atoms from `@/state`.
 */
import { useAtomValue, useSetAtom } from "jotai";
import { chatsAtom, refreshChatsAtom } from "@/state/chats";
import { selectChatAtom, startNewChatAtom } from "@/state/navigation";

export function useChats() {
  const chats = useAtomValue(chatsAtom);
  const refresh = useSetAtom(refreshChatsAtom);
  const selectChat = useSetAtom(selectChatAtom);
  const startNewChat = useSetAtom(startNewChatAtom);

  return {
    chats,
    handleNewChat: () => startNewChat(),
    handleSelectChat: (chatId: string) => selectChat(chatId),
    refresh,
  };
}
