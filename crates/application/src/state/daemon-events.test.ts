import { createStore } from "jotai";
import { expect, test, vi } from "vitest";

const mocks = {
  subscribeEvents: vi.fn(),
};

vi.mock("@/api", () => ({
  daemonApi: {
    subscribeEvents: mocks.subscribeEvents,
  },
}));

const { daemonEventStreamStateAtom, ensureDaemonEventStream } =
  await import("./daemon-events");

test("propagates a disconnect and starts one replacement event stream", async () => {
  const store = createStore();
  let rejectFirst: (reason?: Error) => void = () => undefined;

  mocks.subscribeEvents
    .mockImplementationOnce((_filter, _onEvent, onStatus) => {
      onStatus({ status: "connected" });
      return new Promise<void>((_resolve, reject) => {
        rejectFirst = reject;
      });
    })
    .mockImplementationOnce((_filter, _onEvent, onStatus) => {
      onStatus({ status: "connected" });
      return new Promise<void>(() => undefined);
    });

  ensureDaemonEventStream(store);
  ensureDaemonEventStream(store);

  expect(mocks.subscribeEvents).toHaveBeenCalledTimes(1);
  expect(store.get(daemonEventStreamStateAtom).status).toBe("connected");

  rejectFirst();
  await Promise.resolve();
  await Promise.resolve();

  expect(store.get(daemonEventStreamStateAtom)).toEqual({
    status: "reconnecting",
    error: "undefined",
    reconnectAttempt: 1,
    retryInMs: 250,
  });

  await new Promise((resolve) => setTimeout(resolve, 275));

  expect(mocks.subscribeEvents).toHaveBeenCalledTimes(2);
  expect(store.get(daemonEventStreamStateAtom)).toEqual({
    status: "connected",
    error: null,
    reconnectAttempt: 0,
    retryInMs: null,
  });
});
