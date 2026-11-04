import { createStore, Provider } from "jotai";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test, vi } from "vitest";
vi.mock("@/state/preferences", async () => ({
  defaultAgentIdAtom: (await import("jotai")).atom(""),
}));
import { TooltipProvider } from "@/components/ui/tooltip";
import { AgentSetupButton } from "./agent-setup-button";
import { agentSignInRequiredAtom } from "@/state/daemon-events";

test("installed agents have no setup control until authentication is required", () => {
  const store = createStore();
  const render = () =>
    renderToStaticMarkup(
      <Provider store={store}>
        <TooltipProvider>
          <AgentSetupButton
            agent={{ id: "agent", name: "Test agent" }}
            onOpen={() => {}}
          />
        </TooltipProvider>
      </Provider>,
    );
  expect(render()).toBe("");
  store.set(agentSignInRequiredAtom, { agent: true });
  const html = render();
  expect(html).toContain('aria-label="Sign in to Test agent"');
  expect(html).toContain("<svg");
  expect(html).not.toContain(">Sign in<");
  expect(html).not.toContain("Check setup");
  store.set(agentSignInRequiredAtom, { agent: false });
  expect(render()).toBe("");
});
