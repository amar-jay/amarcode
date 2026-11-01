/// <reference types="vite/client" />
/** A separate build target; desktop IPC remains the default (including tests). */
export const isWeb = import.meta.env.MODE === "web";
