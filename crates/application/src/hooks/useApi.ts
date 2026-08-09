import { useEffect, useState } from "react";

/**
 * Runs a one-shot asynchronous resource request for a component.
 *
 * This is transport-agnostic: it can consume a daemon API call, a plugin call,
 * or any other promise-returning function without embedding product protocol
 * in the UI layer.
 */
export function useApi<T>(request: () => Promise<T>, initial: T): T {
  const [value, setValue] = useState(initial);

  useEffect(() => {
    let active = true;

    void request()
      .then((result) => {
        if (active) setValue(result);
      })
      .catch((error: unknown) => {
        // Presentation belongs to the consumer; this primitive stays generic.
        console.error("asynchronous resource request failed", error);
      });

    return () => {
      active = false;
    };
  }, [request]);

  return value;
}
