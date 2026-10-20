import { atom } from "jotai";

export const WIDTH = 1920;
export const HEIGHT = 1080;
export const FPS = 60;
export const DURATION = 20; // seconds
export const TOTAL_FRAMES = DURATION * FPS;

export const titleAtom = atom({ from: 0, to: 4 });
export const featuresAtom = atom({ from: 4, to: 8 });
export const walkthroughAtom = atom({ from: 8, to: 18.6 });
export const closingAtom = atom({ from: 18.6, to: 20 });

export const captionsAtom = atom([
  { from: 8.6, to: 10.6, text: "Open the agent picker. No extra keys." },
  { from: 10.8, to: 13.0, text: "Switch agents mid-conversation." },
  { from: 13.2, to: 15.2, text: "The context stays with you." },
  { from: 15.4, to: 17.2, text: "Review the files beside the chat." },
  { from: 17.4, to: 18.4, text: "Leave, and the same chat is still here." },
]);
