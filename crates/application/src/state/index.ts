/**
 * Application state — organized by domain.
 *
 * | Domain        | File              | Owns                                              |
 * |---------------|-------------------|---------------------------------------------------|
 * | preferences   | preferences.ts    | theme, palette, default agent (persisted)         |
 * | workspace     | workspace.ts      | active project folder                             |
 * | ui            | ui.ts             | shell chrome (settings dialog)                    |
 * | agents        | agents.ts         | catalog + selected agent                          |
 * | chats         | chats.ts          | sidebar list                                      |
 * | navigation    | navigation.ts     | home vs open chat                                 |
 * | daemon-events | daemon-events.ts  | shared event stream + turn cache                  |
 * | live-chat     | live-chat.ts      | open conversation runtime (run/turn/pending/…)    |
 * | session-config| session-config.ts | last ACP config values per agent                  |
 * | bootstrap     | bootstrap.ts      | one-shot root effects                             |
 */

export * from "./session-config";
export * from "./permission-mode";
export * from "./preferences";
export * from "./workspace";
export * from "./ui";
export * from "./agents";
export * from "./chats";
export * from "./navigation";
export * from "./daemon-events";
export * from "./live-chat";
export { useAppBootstrap } from "./bootstrap";
