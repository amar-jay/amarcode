import { useMemo, useState } from "react";
import { CircleHelp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import type { JsonValue } from "@/types";

export type PendingAgentRequest = {
  kind: "approval" | "input";
  requestId: string;
  details: JsonValue;
};

type Choice = { label: string; value: JsonValue };

type PermissionOption = {
  optionId: string;
  name: string;
  kind: string;
};

type InputRequestPresentation = {
  message: string;
  field?: { key: string; label: string; description?: string; choices: Choice[] };
};

function inputRequestPresentation(details: JsonValue): InputRequestPresentation {
  if (typeof details !== "object" || details === null || Array.isArray(details)) {
    return { message: "The agent needs one detail before it can continue." };
  }

  const record = details as Record<string, unknown>;
  const message = typeof record.message === "string" && record.message.trim()
    ? record.message
    : "The agent needs one detail before it can continue.";
  const schema = record.requestedSchema;
  if (typeof schema !== "object" || schema === null || Array.isArray(schema)) return { message };
  const properties = (schema as Record<string, unknown>).properties;
  if (typeof properties !== "object" || properties === null || Array.isArray(properties)) return { message };

  const key = Object.keys(properties)[0];
  const definition = key ? (properties as Record<string, unknown>)[key] : undefined;
  if (!key || typeof definition !== "object" || definition === null || Array.isArray(definition)) return { message };

  const field = definition as Record<string, unknown>;
  const enumChoices = Array.isArray(field.enum)
    ? field.enum.filter((value): value is string | number | boolean => ["string", "number", "boolean"].includes(typeof value)).map((value) => ({ label: String(value), value }))
    : [];
  const alternatives = [field.oneOf, field.anyOf].find(Array.isArray) as unknown[] | undefined;
  const choices = enumChoices.length > 0 ? enumChoices : (alternatives ?? []).flatMap((option): Choice[] => {
    if (typeof option !== "object" || option === null || Array.isArray(option)) return [];
    const choice = option as Record<string, unknown>;
    const value = choice.const;
    return ["string", "number", "boolean"].includes(typeof value)
      ? [{ label: typeof choice.title === "string" ? choice.title : String(value), value: value as JsonValue }]
      : [];
  });

  return {
    message,
    field: {
      key,
      label: typeof field.title === "string" && field.title.trim() ? field.title : key.replace(/[_-]+/g, " "),
      description: typeof field.description === "string" && field.description.trim() ? field.description : undefined,
      choices,
    },
  };
}

function approvalSummary(details: JsonValue): string {
  if (typeof details === "object" && details !== null && !Array.isArray(details)) {
    const record = details as Record<string, unknown>;
    const toolCall = record.toolCall;
    if (typeof toolCall === "object" && toolCall !== null && !Array.isArray(toolCall)) {
      const tool = toolCall as Record<string, unknown>;
      const title = [tool.title, tool.name]
        .find((value): value is string => typeof value === "string" && Boolean(value.trim()));
      if (title) return title;
    }
    const text = [record.message, record.title, record.reason, record.description]
      .find((value): value is string => typeof value === "string" && Boolean(value.trim()));
    if (text) return text;
  }
  return "Review this request before the agent continues.";
}

function permissionOptions(details: JsonValue): PermissionOption[] {
  if (typeof details !== "object" || details === null || Array.isArray(details)) return [];
  const options = (details as Record<string, unknown>).options;
  if (!Array.isArray(options)) return [];
  return options.flatMap((option): PermissionOption[] => {
    if (typeof option !== "object" || option === null || Array.isArray(option)) return [];
    const record = option as Record<string, unknown>;
    const optionId =
      (typeof record.optionId === "string" && record.optionId) ||
      (typeof record.option_id === "string" && record.option_id) ||
      "";
    if (!optionId) return [];
    return [{
      optionId,
      name: typeof record.name === "string" && record.name.trim() ? record.name : optionId,
      kind: typeof record.kind === "string" ? record.kind : "",
    }];
  });
}

/** ACP `session/request_permission` result for a chosen option. */
function selectedPermissionResult(optionId: string): JsonValue {
  return {
    outcome: {
      outcome: "selected",
      optionId,
    },
  };
}

function isRejectKind(kind: string, optionId: string): boolean {
  return kind.startsWith("reject") || /reject|deny|no/i.test(optionId);
}

function isAllowKind(kind: string, optionId: string): boolean {
  return kind.startsWith("allow") || /allow|yes|approve/i.test(optionId);
}

export function PendingAgentRequestCard({ request, onRespond }: {
  request: PendingAgentRequest;
  onRespond: (result: JsonValue) => Promise<void>;
}) {
  const presentation = useMemo(() => request.kind === "input" ? inputRequestPresentation(request.details) : null, [request]);
  const options = useMemo(
    () => (request.kind === "approval" ? permissionOptions(request.details) : []),
    [request],
  );
  const [answer, setAnswer] = useState("");
  const [choice, setChoice] = useState<JsonValue | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const submit = async (result: JsonValue) => {
    setSubmitting(true);
    try {
      await onRespond(result);
    } finally {
      setSubmitting(false);
    }
  };

  if (request.kind === "approval") {
    const allowOption =
      options.find((option) => isAllowKind(option.kind, option.optionId)) ??
      options.find((option) => !isRejectKind(option.kind, option.optionId));
    const rejectOption = options.find((option) => isRejectKind(option.kind, option.optionId));
    const extraOptions = options.filter(
      (option) => option.optionId !== allowOption?.optionId && option.optionId !== rejectOption?.optionId,
    );

    return (
      <Dialog open>
        <DialogContent showCloseButton={false} className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Approval required</DialogTitle>
            <DialogDescription>{approvalSummary(request.details)}</DialogDescription>
          </DialogHeader>
          <DialogFooter className="flex-col gap-2 sm:flex-col">
            {extraOptions.map((option) => (
              <Button
                key={option.optionId}
                disabled={submitting}
                variant="secondary"
                className="w-full"
                onClick={() => void submit(selectedPermissionResult(option.optionId))}
              >
                {option.name}
              </Button>
            ))}
            <div className="flex w-full flex-col-reverse gap-2 sm:flex-row sm:justify-end">
              <Button
                disabled={submitting}
                onClick={() =>
                  void submit(
                    rejectOption
                      ? selectedPermissionResult(rejectOption.optionId)
                      : selectedPermissionResult("reject-once"),
                  )
                }
                variant="outline"
              >
                {rejectOption?.name ?? "Deny"}
              </Button>
              <Button
                disabled={submitting}
                onClick={() =>
                  void submit(
                    allowOption
                      ? selectedPermissionResult(allowOption.optionId)
                      : selectedPermissionResult("allow-once"),
                  )
                }
              >
                {allowOption?.name ?? "Allow"}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  const field = presentation?.field;
  const selectedAnswer = choice ?? answer.trim();
  const canSubmit = typeof selectedAnswer === "string" ? Boolean(selectedAnswer) : selectedAnswer !== null;

  return (
    <Dialog open>
      <DialogContent showCloseButton={false} className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <CircleHelp className="size-4 text-primary" />
            The agent needs your input
            {presentation?.field?.label && <span className="font-medium capitalize">({presentation.field.label})</span>}
          </DialogTitle>
          <DialogDescription className="whitespace-pre-wrap">{presentation?.field?.description || presentation?.message}</DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          {field && field.choices.length > 0 ? (
            <RadioGroup
              value={choice === null ? undefined : String(field.choices.findIndex((item) => item.value === choice))}
              onValueChange={(value) => setChoice(field.choices[Number(value)]?.value ?? null)}
              className="pt-1"
            >
              {field.choices.map((item, index) => {
                const id = `${request.requestId}-choice-${index}`;
                return (
                  <label key={`${item.label}-${String(item.value)}`} htmlFor={id} className="flex cursor-pointer items-center gap-2 rounded-md border border-input px-3 py-2 text-sm hover:bg-accent">
                    <RadioGroupItem id={id} value={String(index)} />
                    {item.label}
                  </label>
                );
              })}
            </RadioGroup>
          ) : (
            <Input value={answer} onChange={(event) => setAnswer(event.target.value)} placeholder="Type your answer" />
          )}
        </div>
        <DialogFooter>
          <Button
            disabled={!canSubmit || submitting}
            onClick={() => {
              const content = field ? { [field.key]: selectedAnswer } : { answer: selectedAnswer };
              void submit({ action: "accept", content });
            }}
          >
            {submitting ? "Sending…" : "Continue"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
