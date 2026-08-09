import { AgentDefinition } from "@/types";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { FolderOpen, Plus, Bot } from "lucide-react";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "./ui/card";

export function NewSessionCard({
  agent,
  workspace,
  agents,
  selectedAgent,
  showAgentForm,
  agentForm,
  error,
  isLaunching,
  onChooseWorkspace,
  onSelectAgent,
  onShowAgentForm,
  onAgentFormChange,
  onAddAgent,
  onStart,
}: {
  agent?: AgentDefinition;
  workspace: string;
  agents: AgentDefinition[];
  selectedAgent: string;
  showAgentForm: boolean;
  agentForm: { name: string; command: string; arguments: string };
  error: string;
  isLaunching: boolean;
  onChooseWorkspace: () => void;
  onSelectAgent: (agentId: string) => void;
  onShowAgentForm: () => void;
  onAgentFormChange: (form: {
    name: string;
    command: string;
    arguments: string;
  }) => void;
  onAddAgent: (event: React.FormEvent) => void;
  onStart: () => void;
}) {
  return (
    <section className="m-auto w-full max-w-2xl px-8 py-12">
      <Card className="shadow-sm">
        <CardHeader>
          <p className="text-[11px] font-medium uppercase tracking-[.12em] text-primary">
            New agent session
          </p>
          <CardTitle className="text-3xl font-medium tracking-tight">
            A quiet place for capable agents.
          </CardTitle>
          <CardDescription className="max-w-xl text-sm leading-6">
            Choose a local project, start any ACP-compatible coding agent, and
            review its work in a focused desktop workspace.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-5">
          <label className="grid gap-2 text-xs font-medium">
            Project folder
            <div className="flex gap-2">
              <Input
                value={workspace}
                placeholder="Choose a local project"
                readOnly
              />
              <Button
                type="button"
                variant="outline"
                onClick={onChooseWorkspace}
              >
                <FolderOpen data-icon="inline-start" />
                Browse
              </Button>
            </div>
          </label>
          <label className="grid gap-2 text-xs font-medium">
            ACP agent
            <select
              className="h-8 rounded-md border border-input bg-input/20 px-2 text-xs outline-none focus:ring-2 focus:ring-ring/30"
              value={selectedAgent}
              onChange={(event) => onSelectAgent(event.target.value)}
            >
              {agents.map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.name}
                </option>
              ))}
            </select>
          </label>
          <Button
            type="button"
            variant="link"
            className="w-fit px-0"
            onClick={onShowAgentForm}
          >
            <Plus data-icon="inline-start" />
            Add custom ACP command
          </Button>
          {showAgentForm && (
            <form
              className="grid gap-2 rounded-md border border-border bg-muted/30 p-3"
              onSubmit={onAddAgent}
            >
              <Input
                required
                placeholder="Display name"
                value={agentForm.name}
                onChange={(event) =>
                  onAgentFormChange({ ...agentForm, name: event.target.value })
                }
              />
              <Input
                required
                placeholder="Executable, e.g. my-agent"
                value={agentForm.command}
                onChange={(event) =>
                  onAgentFormChange({
                    ...agentForm,
                    command: event.target.value,
                  })
                }
              />
              <Input
                placeholder="Arguments, space-separated"
                value={agentForm.arguments}
                onChange={(event) =>
                  onAgentFormChange({
                    ...agentForm,
                    arguments: event.target.value,
                  })
                }
              />
              <Button className="w-fit" type="submit">
                Add agent
              </Button>
            </form>
          )}
          <Button
            size="lg"
            className="w-fit"
            disabled={!workspace || !agent || isLaunching}
            onClick={onStart}
          >
            <Bot data-icon="inline-start" />
            {isLaunching
              ? "Starting session…"
              : `Launch ${agent?.name ?? "agent"}`}
          </Button>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </CardContent>
      </Card>
    </section>
  );
}
