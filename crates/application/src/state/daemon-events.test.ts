import { createStore } from "jotai";
import { afterAll, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  subscribeEvents: vi.fn(),
}));

vi.mock("@/api", () => ({
  daemonApi: {
    subscribeEvents: mocks.subscribeEvents,
  },
}));

import {
  daemonEventStreamStateAtom,
  ensureDaemonEventStream,
} from "./daemon-events";

afterAll(() => {
  vi.useRealTimers();
});

test("propagates a disconnect and starts one replacement event stream", async () => {
  vi.useFakeTimers();
  const store = createStore();
  let rejectFirst: (reason: Error) => void = () => undefined;

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

  rejectFirst(new Error("daemon closed the subscription connection"));
  await Promise.resolve();
  await Promise.resolve();

  expect(store.get(daemonEventStreamStateAtom)).toEqual({
    status: "reconnecting",
    error: "daemon closed the subscription connection",
    reconnectAttempt: 1,
    retryInMs: 250,
  });

  await vi.advanceTimersByTimeAsync(250);

  expect(mocks.subscribeEvents).toHaveBeenCalledTimes(2);
  expect(store.get(daemonEventStreamStateAtom)).toEqual({
    status: "connected",
    error: null,
    reconnectAttempt: 0,
    retryInMs: null,
  });
});
