import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PROTOCOL_VERSION } from "@/generated/protocol";
import {
  browserConnection,
  browserEvents,
  browserInvoke,
  browserRpc,
  connectBrowserDaemon,
  disconnectBrowserDaemon,
  isShortRemoteLink,
  normalizeRemoteUrl,
  resolveRemoteUrl,
} from "./browser-daemon";
class Socket {
  static instances: Socket[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  closed = false;
  constructor(
    public url: string,
    public protocols: string[],
  ) {
    Socket.instances.push(this);
    queueMicrotask(() => this.onopen?.());
  }
  receive(value: unknown) {
    this.onmessage?.({ data: `${JSON.stringify(value)}\n` });
  }
  send(data: string) {
    const request = JSON.parse(data);
    requests.push(request);
    queueMicrotask(() => {
      if (request.method === "prompt") return;
      if (request.method === "subscribe_events") {
        this.receive({ result: { subscribed: true } });
        this.receive({
          event: { type: "chatUpdated", payload: { chat_id: "chat" } },
        });
      } else
        this.receive({
          result:
            request.method === "health"
              ? {
                  remote_auth_required: true,
                  protocol_version: PROTOCOL_VERSION,
                  version: "1",
                }
              : request.method === "list_agents"
                ? { agents: [{ id: "agent" }] }
                : { ok: true },
        });
    });
  }
  close() {
    if (this.closed) return;
    this.closed = true;
    queueMicrotask(() => this.onclose?.());
  }
}
let requests: { method: string; params: unknown }[] = [];
let storage: Map<string, string>;
const token = "a".repeat(64);
beforeEach(() => {
  requests = [];
  Socket.instances = [];
  storage = new Map();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
  vi.stubGlobal("WebSocket", Socket);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.startsWith("https://updates.amarcode.amarjay.com/r/")) {
        return {
          ok: true,
          json: async () => ({
            url: "wss://my-remote.trycloudflare.com/rpc",
          }),
        };
      }
      return {
        ok: true,
        json: async () => ({ ticket: "b".repeat(64), web_api_version: 1 }),
      };
    }),
  );
});
afterEach(() => {
  disconnectBrowserDaemon();
  vi.unstubAllGlobals();
});
describe("remote browser transport", () => {
  it("accepts only secure daemon endpoints without credentials or token URLs", () => {
    expect(normalizeRemoteUrl("https://daemon.example")).toBe(
      "wss://daemon.example/rpc",
    );
    for (const bad of [
      "ws://daemon.example",
      "http://daemon.example",
      "wss://user:secret@daemon.example",
      "https://daemon.example/rpc?token=secret",
      "https://daemon.example/#secret",
      "https://daemon.example/other",
    ])
      expect(() => normalizeRemoteUrl(bad)).toThrow();
  });
  it("resolves branded short links before connecting", async () => {
    const short = "https://updates.amarcode.amarjay.com/r/abcDEF123_-";
    expect(isShortRemoteLink(short)).toBe(true);
    expect(await resolveRemoteUrl(short)).toBe(
      "wss://my-remote.trycloudflare.com/rpc",
    );
    await connectBrowserDaemon(short, token);
    expect(storage.get("amarcode-web-url")).toBe(
      "wss://my-remote.trycloudflare.com/rpc",
    );
    expect(fetch).toHaveBeenCalledWith(
      short,
      expect.objectContaining({
        redirect: "error",
        credentials: "omit",
        headers: { accept: "application/json" },
      }),
    );
  });
  it("rejects resolver errors and destinations outside quick tunnels", async () => {
    const short = "https://updates.amarcode.amarjay.com/r/abcDEF123_-";
    for (const destination of [
      "wss://evil.example/rpc",
      "wss://host.trycloudflare.com:8443/rpc",
      "wss://host.trycloudflare.com/rpc?token=secret",
      "wss://host.trycloudflare.com/terminal",
    ]) {
      vi.mocked(fetch).mockResolvedValueOnce({
        ok: true,
        json: async () => ({ url: destination }),
      } as Response);
      await expect(resolveRemoteUrl(short)).rejects.toThrow();
    }
    vi.mocked(fetch).mockResolvedValueOnce({ ok: false } as Response);
    await expect(resolveRemoteUrl(short)).rejects.toThrow("invalid");
    vi.mocked(fetch).mockRejectedValueOnce(new Error("offline"));
    await expect(resolveRemoteUrl(short)).rejects.toThrow("network");
    expect(Socket.instances).toHaveLength(0);
  });
  it("authenticates with tickets and stores no token", async () => {
    await connectBrowserDaemon("https://daemon.example", token);
    expect(browserConnection().hasToken).toBe(true);
    expect([...storage.keys()].sort()).toEqual([
      "amarcode-connection-scope",
      "amarcode-web-url",
    ]);
    expect([...storage.values()].join()).not.toContain(token);
    expect(fetch).toHaveBeenCalledWith(
      new URL("https://daemon.example/web/session"),
      expect.objectContaining({
        method: "POST",
        redirect: "error",
        credentials: "omit",
        headers: { Authorization: `Bearer ${token}` },
      }),
    );
    expect(Socket.instances[0].url).not.toContain(token);
    expect(Socket.instances[0].protocols).toEqual([
      "amarcode.rpc.v1",
      `amarcode.ticket.${"b".repeat(64)}`,
    ]);
  });
  it("maps adapter inputs and unwraps results while preserving structured protocol values", async () => {
    await connectBrowserDaemon("wss://daemon.example/rpc", token);
    expect(await browserInvoke("list_agents")).toEqual([{ id: "agent" }]);
    await browserInvoke("agent_setup", {
      agentId: "agent",
      action: "sign_in",
      methodId: "browser",
    });
    expect(requests.at(-1)).toEqual({
      method: "agent_setup",
      params: { agent_id: "agent", action: "sign_in", method_id: "browser" },
    });
    const params = { chat_id: "chat", prompt: { text: "Follow up" } };
    await browserInvoke("enqueue_prompt", { params });
    expect(requests.at(-1)).toEqual({ method: "enqueue_prompt", params });
    await browserInvoke("read_workspace_file", {
      workspacePath: "/remote",
      relativePath: "a.txt",
    });
    expect(requests.at(-1)?.params).toEqual({
      workspace_path: "/remote",
      relative_path: "a.txt",
    });
  });
  it("cancels independently of a long prompt and never retries uncertain delivery", async () => {
    await connectBrowserDaemon("https://daemon.example", token);
    const pending = browserRpc("prompt", { text: "Work" }).catch(
      (error: Error) => error,
    );
    await browserRpc("cancel", { chat_id: "chat" });
    expect(requests.filter((r) => r.method === "prompt")).toHaveLength(1);
    disconnectBrowserDaemon();
    expect(await pending).toBeInstanceOf(Error);
    expect(browserConnection().hasToken).toBe(false);
    await expect(browserRpc("health")).rejects.toThrow(
      "Connect to a remote daemon first",
    );
  });
  it("streams events and rejects on disconnect so the existing recovery loop reconnects", async () => {
    await connectBrowserDaemon("https://daemon.example", token);
    const onEvent = vi.fn();
    const onStatus = vi.fn();
    const pending = browserEvents({}, onEvent, onStatus).catch(
      (error: Error) => error,
    );
    await vi.waitFor(() =>
      expect(onEvent).toHaveBeenCalledWith({
        type: "chatUpdated",
        payload: { chat_id: "chat" },
      }),
    );
    expect(onStatus).toHaveBeenCalledWith({ status: "connected" });
    disconnectBrowserDaemon();
    expect(await pending).toBeInstanceOf(Error);
  });
  it("rejects incompatible web APIs before opening a socket and forgets the token", async () => {
    vi.mocked(fetch).mockResolvedValueOnce({
      ok: true,
      json: async () => ({ ticket: "b".repeat(64), web_api_version: 2 }),
    } as Response);
    await expect(
      connectBrowserDaemon("https://daemon.example", token),
    ).rejects.toThrow("Incompatible");
    expect(Socket.instances).toHaveLength(0);
    expect(browserConnection().hasToken).toBe(false);
  });
});
