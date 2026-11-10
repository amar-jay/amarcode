import { MAX_RPC_MESSAGE_BYTES, PROTOCOL_VERSION } from "@/generated/protocol";
import type { EditorEvent, Health } from "@/types";

let target = "";
let secret = "";
const sockets = new Set<WebSocket>();
let generation = 0;

export function normalizeRemoteUrl(input: string) {
  const url = new URL(input.trim());
  if (url.protocol === "https:") url.protocol = "wss:";
  if (
    url.protocol !== "wss:" ||
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    !["/", "/rpc", ""].includes(url.pathname)
  )
    throw new Error(
      "Enter a secure HTTPS or WSS daemon URL without credentials or query parameters.",
    );
  url.pathname = "/rpc";
  return url.toString();
}

export function disconnectBrowserDaemon() {
  generation++;
  secret = "";
  for (const socket of sockets) socket.close();
  sockets.clear();
}
export function browserConnection() {
  return {
    remote: true,
    mode: "websocket" as const,
    url: target,
    hasToken: Boolean(secret),
    ssh: null,
    scope: localStorage.getItem("amarcode-connection-scope") ?? "",
  };
}
async function socket() {
  const expected = generation;
  if (!secret) throw new Error("Connect to a remote daemon first.");
  const endpoint = new URL(target);
  endpoint.protocol = "https:";
  endpoint.pathname = "/web/session";
  let response: Response;
  try {
    response = await fetch(endpoint, {
      method: "POST",
      headers: { Authorization: `Bearer ${secret}` },
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
      signal: AbortSignal.timeout(10_000),
    });
  } catch {
    throw new Error(
      "Cannot reach this daemon. Check its URL, connection, and browser origin configuration; an older daemon may need updating.",
    );
  }
  if (!response.ok)
    throw new Error(
      response.status === 401
        ? "Invalid access token."
        : response.status === 426
          ? "This daemon requires a paired native client for end-to-end encryption."
          : "Browser access is unavailable. Update the daemon and check its browser origin configuration.",
    );
  const ticket = await response.json();
  if (
    ticket.web_api_version !== 1 ||
    typeof ticket.ticket !== "string" ||
    !/^[a-f0-9]{64}$/.test(ticket.ticket)
  )
    throw new Error(
      "Incompatible daemon web API. Update the daemon and reload this page.",
    );
  if (expected !== generation || !secret)
    throw new Error("Connection changed.");
  return await new Promise<WebSocket>((resolve, reject) => {
    const ws = new WebSocket(target, [
      "amarcode.rpc.v1",
      `amarcode.ticket.${ticket.ticket}`,
    ]);
    sockets.add(ws);
    const timer = setTimeout(() => {
      ws.close();
      reject(new Error("Connection timed out."));
    }, 10_000);
    ws.onopen = () => {
      clearTimeout(timer);
      if (expected !== generation) {
        ws.close();
        reject(new Error("Connection changed."));
      } else resolve(ws);
    };
    ws.onerror = () => {
      clearTimeout(timer);
      reject(new Error("Could not authenticate the daemon connection."));
    };
    ws.onclose = () => {
      clearTimeout(timer);
      sockets.delete(ws);
      reject(new Error("Daemon connection closed."));
    };
  });
}
function lines(data: unknown): unknown[] {
  if (
    typeof data !== "string" ||
    new TextEncoder().encode(data).byteLength > MAX_RPC_MESSAGE_BYTES
  )
    throw new Error("Invalid daemon response.");
  return data
    .split("\n")
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line));
}
/** One request per socket: long prompts cannot serialize Stop or approval RPCs. */
export async function browserRpc<T>(
  method: string,
  params: unknown = {},
): Promise<T> {
  const ws = await socket();
  return await new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => {
        ws.close();
        reject(
          new Error(
            "Daemon request timed out; delivery may have completed. Check the chat before retrying.",
          ),
        );
      },
      method === "prompt" ? 24 * 60 * 60 * 1000 : 120_000,
    );
    const finish = () => {
      clearTimeout(timer);
      ws.close();
    };
    ws.onclose = () => {
      sockets.delete(ws);
      clearTimeout(timer);
      reject(
        new Error(
          "Connection lost; delivery may have completed. Reconnect before retrying.",
        ),
      );
    };
    ws.onerror = () => {
      finish();
      reject(new Error("Daemon request failed."));
    };
    ws.onmessage = (event) => {
      try {
        for (const value of lines(event.data)) {
          const reply = value as { result?: T; error?: string };
          if (reply.error) {
            reject(new Error(reply.error));
            finish();
            return;
          }
          if (Object.hasOwn(reply, "result")) {
            resolve(reply.result as T);
            finish();
            return;
          }
        }
      } catch {
        finish();
        reject(new Error("Invalid daemon response."));
      }
    };
    ws.send(`${JSON.stringify({ method, params })}\n`);
  });
}
export async function connectBrowserDaemon(url: string, token: string) {
  const normalized = normalizeRemoteUrl(url);
  if (!/^[A-Za-z0-9_-]{32,512}$/.test(token.trim()))
    throw new Error("Enter a valid daemon access token.");
  disconnectBrowserDaemon();
  target = normalized;
  secret = token.trim();
  try {
    const health = await browserRpc<Health>("health");
    if (
      !health.remote_auth_required ||
      health.protocol_version !== PROTOCOL_VERSION
    )
      throw new Error(
        "Incompatible or unauthenticated daemon. Update the daemon before connecting.",
      );
    const hash = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(normalized),
    );
    localStorage.setItem(
      "amarcode-connection-scope",
      `web-${Array.from(new Uint8Array(hash), (b) => b.toString(16).padStart(2, "0")).join("")}`,
    );
    // Remember only the public address. Credentials never enter browser storage.
    localStorage.setItem("amarcode-web-url", normalized);
    return health;
  } catch (error) {
    disconnectBrowserDaemon();
    throw error;
  }
}
export async function browserEvents(
  filter: unknown,
  onEvent: (event: EditorEvent) => void,
  onStatus: (
    status: { status: "connected" } | { status: "disconnected"; error: string },
  ) => void,
): Promise<void> {
  const ws = await socket();
  return await new Promise((_, reject) => {
    let acknowledged = false;
    const timer = setTimeout(() => {
      ws.close();
    }, 10_000);
    ws.onmessage = (event) => {
      try {
        for (const value of lines(event.data)) {
          const reply = value as {
            result?: { subscribed: boolean };
            event?: EditorEvent;
            error?: string;
          };
          if (reply.error) throw new Error(reply.error);
          if (reply.result?.subscribed) {
            acknowledged = true;
            clearTimeout(timer);
            onStatus({ status: "connected" });
          } else if (acknowledged && reply.event) onEvent(reply.event);
        }
      } catch {
        ws.close();
      }
    };
    ws.onerror = () => ws.close();
    ws.onclose = () => {
      clearTimeout(timer);
      sockets.delete(ws);
      onStatus({ status: "disconnected", error: "Daemon connection lost." });
      reject(new Error("Daemon connection lost."));
    };
    ws.send(
      `${JSON.stringify({ method: "subscribe_events", params: filter })}\n`,
    );
  });
}

const methods: Record<string, string> = {
  daemon_health: "health",
  daemon_version: "version",
  get_amarcode_acp_config: "get_amarcode_acp_config",
};
export async function browserInvoke<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (command === "get_daemon_connection") return browserConnection() as T;
  if (command === "get_daemon_connection_status")
    return {
      status: secret ? "connected" : "idle",
      error: null,
      reconnectAttempt: 0,
      retryInMs: null,
      generation,
    } as T;
  if (command === "daemon_bootstrap") {
    const health = await browserRpc<Health>("health");
    (args.onStatus as { onmessage?: (value: unknown) => void })?.onmessage?.({
      status: "ready",
      version: health.version,
    });
    return health as T;
  }
  if (
    [
      "daemon_install",
      "daemon_check_update",
      "daemon_update",
      "remove_local_daemon",
      "app_check_update",
      "app_install_update",
      "set_daemon_connection",
      "exit_application",
    ].includes(command)
  )
    throw new Error("This action is available in the desktop application.");
  const params =
    args.params ??
    Object.fromEntries(
      Object.entries(args).map(([key, value]) => [
        key.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`),
        value,
      ]),
    );
  const value = await browserRpc<unknown>(methods[command] ?? command, params);
  const wrappers: Record<string, string> = {
    list_agents: "agents",
    list_chats: "chats",
    list_acp_events_for_chat: "events",
    list_agent_runs_for_chat: "runs",
  };
  return (
    wrappers[command]
      ? (value as Record<string, unknown>)[wrappers[command]]
      : value
  ) as T;
}
