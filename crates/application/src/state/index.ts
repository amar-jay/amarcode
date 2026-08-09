/**
 * Application state — organized by domain.
 *
 * | Domain        | File              | Owns                                              |
 * |---------------|-------------------|---------------------------------------------------|
 * | preferences   | preferences.ts    | theme, palette, default agent/mode (persisted)    |
 * | workspace     | workspace.ts      | active project folder                             |
 * | ui            | ui.ts             | shell chrome (settings dialog)                    |
 * | agents        | agents.ts         | catalog + selected agent                          |
 * | chats         | chats.ts          | sidebar list                                      |
 * | navigation    | navigation.ts     | home vs open chat                                 |
 * | daemon-events | daemon-events.ts  | shared event stream + turn cache                  |
 * | live-chat     | live-chat.ts      | open conversation runtime (run/turn/pending/…)    |
 * | session-mode  | session-mode.ts   | plan | build | ask                                |
 * | bootstrap     | bootstrap.ts      | one-shot root effects                             |
 */

export * from "./session-mode";
export * from "./preferences";
export * from "./workspace";
export * from "./ui";
export * from "./agents";
export * from "./chats";
export * from "./navigation";
export * from "./daemon-events";
export * from "./live-chat";
export { useAppBootstrap } from "./bootstrap";
