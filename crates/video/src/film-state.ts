import { atom } from "jotai";

export const captionsAtom = atom([
  { from: 0.8, to: 3.4, text: "The conversation, the commands, and the file changes." },
  { from: 3.6, to: 6.8, text: "Git history stays in the same chat." },
  { from: 7.2, to: 10.4, text: "Several agents. The subscriptions you already use." },
  { from: 10.6, to: 16.0, text: "Switch in the middle. The context stays." },
  { from: 16.4, to: 22.4, text: "Review the files beside the conversation." },
  { from: 22.6, to: 26.0, text: "Leave, and the same chat is still here." },
]);

export const splashAtAtom = atom(26.3);
