import { atom } from "jotai";
import { daemonApi } from "@/api";
import type { Chat } from "@/types";

/** Sidebar chat list (summaries only). */
export const chatsAtom = atom<Chat[]>([]);

/** Coalesce concurrent listChats RPCs. */
let refreshInFlight: Promise<Chat[]> | null = null;

export const refreshChatsAtom = atom(null, async (_get, set) => {
  if (refreshInFlight) return refreshInFlight;
  refreshInFlight = daemonApi
    .listChats()
    .then((chats) => {
      set(chatsAtom, chats);
      return chats;
    })
    .finally(() => {
      refreshInFlight = null;
    });
  return refreshInFlight;
});
