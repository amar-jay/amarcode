import { useEffect, useMemo, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { cn } from "@/lib/utils";
import { notifyAttention } from "@/lib/notify";
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

type ActionSummary = {
  title: string;
  command?: string;
  cwd?: string;
  path?: string;
};

type InputRequestPresentation = {
  message: string;
  field?: {
    key: string;
    label: string;
    description?: string;
    choices: Choice[];
  };
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    return null;
  return value as Record<string, unknown>;
}

function inputRequestPresentation(
  details: JsonValue,
): InputRequestPresentation {
  const record = asRecord(details);
  if (!record) {
    return { message: "The agent needs one detail before it can continue." };
  }

  const message =
    typeof record.message === "string" && record.message.trim()
      ? record.message
      : "The agent needs one detail before it can continue.";

  const schema = asRecord(record.requestedSchema);
  const properties = schema ? asRecord(schema.properties) : null;
  if (!properties) return { message };

  const key = Object.keys(properties)[0];
  const field = key ? asRecord(properties[key]) : null;
  if (!key || !field) return { message };

  const enumChoices = Array.isArray(field.enum)
    ? field.enum
        .filter((value): value is string | number | boolean =>
          ["string", "number", "boolean"].includes(typeof value),
        )
        .map((value) => ({ label: String(value), value }))
    : [];

  const alternatives = [field.oneOf, field.anyOf].find(Array.isArray) as
    unknown[] | undefined;
  const choices =
    enumChoices.length > 0
      ? enumChoices
      : (alternatives ?? []).flatMap((option): Choice[] => {
          const choice = asRecord(option);
          if (!choice) return [];
          const value = choice.const;
          return ["string", "number", "boolean"].includes(typeof value)
            ? [
                {
                  label:
                    typeof choice.title === "string"
                      ? choice.title
                      : String(value),
                  value: value as JsonValue,
                },
              ]
            : [];
        });

  return {
    message,
    field: {
      key,
      label:
        typeof field.title === "string" && field.title.trim()
          ? field.title
          : key.replace(/[_-]+/g, " "),
      description:
        typeof field.description === "string" && field.description.trim()
          ? field.description
          : undefined,
      choices,
    },
  };
}

function actionSummary(details: JsonValue): ActionSummary | null {
  const record = asRecord(details);
  if (!record) return null;

  const tool = asRecord(record.toolCall) ?? record;
  const title =
    [tool.title, tool.name, record.message, record.title].find(
      (value): value is string =>
        typeof value === "string" && Boolean(value.trim()),
    ) ?? "Agent action";

  const rawInput =
    asRecord(tool.rawInput) ??
    asRecord(tool.input) ??
    asRecord(record.rawInput);
  const command =
    (rawInput && typeof rawInput.command === "string" && rawInput.command) ||
    (typeof tool.command === "string" && tool.command) ||
    undefined;
  const cwd =
    (rawInput && typeof rawInput.cwd === "string" && rawInput.cwd) ||
    (typeof tool.cwd === "string" && tool.cwd) ||
    undefined;

  let path: string | undefined;
  const locations = Array.isArray(tool.locations) ? tool.locations : [];
  for (const location of locations) {
    const loc = asRecord(location);
    if (loc && typeof loc.path === "string") {
      path = loc.path;
      break;
    }
  }
  if (!path && typeof tool.path === "string") path = tool.path;

  return { title, command, cwd, path };
}

function permissionOptions(details: JsonValue): PermissionOption[] {
  const record = asRecord(details);
  if (!record || !Array.isArray(record.options)) return [];
  return record.options.flatMap((option): PermissionOption[] => {
    const item = asRecord(option);
    if (!item) return [];
    const optionId =
      (typeof item.optionId === "string" && item.optionId) ||
      (typeof item.option_id === "string" && item.option_id) ||
      "";
    if (!optionId) return [];
    return [
      {
        optionId,
        name:
          typeof item.name === "string" && item.name.trim()
            ? item.name
            : optionId,
        kind: typeof item.kind === "string" ? item.kind : "",
      },
    ];
  });
}

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

function defaultOptionId(options: PermissionOption[]): string {
  const allowOnce = options.find(
    (option) =>
      option.kind === "allow_once" || /allow[-_]?once/i.test(option.optionId),
  );
  if (allowOnce) return allowOnce.optionId;
  const firstAllow = options.find((option) =>
    isAllowKind(option.kind, option.optionId),
  );
  return firstAllow?.optionId ?? options[0]?.optionId ?? "";
}

export function PendingAgentRequestCard({
  request,
  onRespond,
}: {
  request: PendingAgentRequest;
  onRespond: (result: JsonValue) => Promise<void>;
}) {
  const isApproval = request.kind === "approval";
  useEffect(() => {
    const title = isApproval ? "Permission required" : "Input needed";
    const body = isApproval
      ? "Amarcode is waiting for your approval."
      : "Amarcode is waiting for your response.";
    notifyAttention(`agent-request:${request.requestId}`, title, body);
  }, [isApproval, request.requestId]);
  const presentation = useMemo(
    () => (isApproval ? null : inputRequestPresentation(request.details)),
    [isApproval, request.details],
  );
  const options = useMemo(
    () => (isApproval ? permissionOptions(request.details) : []),
    [isApproval, request.details],
  );
  const action = useMemo(
    () => (isApproval ? actionSummary(request.details) : null),
    [isApproval, request.details],
  );

  const orderedOptions = useMemo(() => {
    if (options.length === 0) {
      return [
        { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
        { optionId: "reject-once", name: "Reject", kind: "reject_once" },
      ] satisfies PermissionOption[];
    }
    return [...options].sort((left, right) => {
      const rank = (option: PermissionOption) => {
        if (isRejectKind(option.kind, option.optionId)) return 2;
        if (isAllowKind(option.kind, option.optionId)) return 0;
        return 1;
      };
      return rank(left) - rank(right);
    });
  }, [options]);

  const [selectedOptionId, setSelectedOptionId] = useState(() =>
    defaultOptionId(orderedOptions),
  );
  const [answer, setAnswer] = useState("");
  const [choice, setChoice] = useState<JsonValue | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Keep selection valid when the request changes mid-stream.
  const activeOptionId = orderedOptions.some(
    (option) => option.optionId === selectedOptionId,
  )
    ? selectedOptionId
    : defaultOptionId(orderedOptions);

  const submit = async (result: JsonValue) => {
    setSubmitting(true);
    try {
      await onRespond(result);
    } finally {
      setSubmitting(false);
    }
  };

  if (isApproval) {
    const summaryLine =
      action?.command ?? action?.path ?? action?.title ?? "Agent action";

    return (
      <Dialog open>
        <DialogContent
          showCloseButton={false}
          className="gap-4 sm:max-w-sm"
          onPointerDownOutside={(event) => event.preventDefault()}
          onEscapeKeyDown={(event) => event.preventDefault()}
        >
          <DialogHeader className="space-y-1.5 text-left">
            <DialogTitle>Permission required</DialogTitle>
            <DialogDescription>
              Choose how to handle this agent action.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-1 rounded-md border bg-muted/40 px-3 py-2">
            <p className="font-mono text-xs leading-relaxed break-all text-foreground">
              {summaryLine}
            </p>
            {action?.cwd && (
              <p className="truncate font-mono text-[0.7rem] text-muted-foreground">
                {action.cwd}
              </p>
            )}
          </div>

          <RadioGroup
            value={activeOptionId}
            onValueChange={setSelectedOptionId}
            className="gap-2"
            disabled={submitting}
          >
            {orderedOptions.map((option) => {
              const id = `${request.requestId}-${option.optionId}`;
              const reject = isRejectKind(option.kind, option.optionId);
              return (
                <div
                  key={option.optionId}
                  className={cn(
                    "flex items-center gap-3 rounded-md border px-3 py-2",
                    activeOptionId === option.optionId
                      ? reject
                        ? "border-destructive/40 bg-destructive/5"
                        : "border-primary/40 bg-primary/5"
                      : "border-border bg-background",
                  )}
                >
                  <RadioGroupItem id={id} value={option.optionId} />
                  <Label
                    htmlFor={id}
                    className={cn(
                      "flex-1 cursor-pointer text-sm font-normal",
                      reject && "text-destructive",
                    )}
                  >
                    {option.name}
                  </Label>
                </div>
              );
            })}
          </RadioGroup>

          <DialogFooter>
            <Button
              disabled={!activeOptionId || submitting}
              onClick={() =>
                void submit(selectedPermissionResult(activeOptionId))
              }
            >
              {submitting ? (
                <>
                  <LoaderCircle className="size-3.5 animate-spin" />
                  Sending…
                </>
              ) : (
                "Continue"
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  const field = presentation?.field;
  const selectedAnswer = choice ?? answer.trim();
  const canSubmit =
    typeof selectedAnswer === "string"
      ? Boolean(selectedAnswer)
      : selectedAnswer !== null;

  return (
    <Dialog open>
      <DialogContent
        showCloseButton={false}
        className="gap-4 sm:max-w-sm"
        onPointerDownOutside={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => event.preventDefault()}
      >
        <DialogHeader className="space-y-1.5 text-left">
          <DialogTitle>Input needed</DialogTitle>
          <DialogDescription className="whitespace-pre-wrap">
            {presentation?.message}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          {field?.label && (
            <div className="space-y-1">
              <p className="text-sm font-medium capitalize">{field.label}</p>
              {field.description && (
                <p className="text-xs text-muted-foreground whitespace-pre-wrap">
                  {field.description}
                </p>
              )}
            </div>
          )}

          {field && field.choices.length > 0 ? (
            <RadioGroup
              value={
                choice === null
                  ? undefined
                  : String(
                      field.choices.findIndex((item) => item.value === choice),
                    )
              }
              onValueChange={(value) =>
                setChoice(field.choices[Number(value)]?.value ?? null)
              }
              className="gap-2"
              disabled={submitting}
            >
              {field.choices.map((item, index) => {
                const id = `${request.requestId}-choice-${index}`;
                const selected = choice === item.value;
                return (
                  <div
                    key={`${item.label}-${String(item.value)}`}
                    className={cn(
                      "flex items-center gap-3 rounded-md border px-3 py-2",
                      selected
                        ? "border-primary/40 bg-primary/5"
                        : "border-border bg-background",
                    )}
                  >
                    <RadioGroupItem id={id} value={String(index)} />
                    <Label
                      htmlFor={id}
                      className="flex-1 cursor-pointer text-sm font-normal"
                    >
                      {item.label}
                    </Label>
                  </div>
                );
              })}
            </RadioGroup>
          ) : (
            <Input
              value={answer}
              onChange={(event) => setAnswer(event.target.value)}
              placeholder="Type your answer"
              className="h-9"
              autoFocus
              disabled={submitting}
              onKeyDown={(event) => {
                if (event.key === "Enter" && canSubmit && !submitting) {
                  event.preventDefault();
                  const content = field
                    ? { [field.key]: selectedAnswer }
                    : { answer: selectedAnswer };
                  void submit({ action: "accept", content });
                }
              }}
            />
          )}
        </div>

        <DialogFooter>
          <Button
            disabled={!canSubmit || submitting}
            onClick={() => {
              const content = field
                ? { [field.key]: selectedAnswer }
                : { answer: selectedAnswer };
              void submit({ action: "accept", content });
            }}
          >
            {submitting ? (
              <>
                <LoaderCircle className="size-3.5 animate-spin" />
                Sending…
              </>
            ) : (
              "Continue"
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
