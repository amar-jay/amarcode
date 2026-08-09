import { useMemo, useState } from "react";
import {
  Check,
  CircleHelp,
  FileCode2,
  LoaderCircle,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  Terminal,
  Wrench,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
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
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
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

type ToolCallSummary = {
  title: string;
  kind?: string;
  status?: string;
  toolCallId?: string;
  detailLines: string[];
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
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function inputRequestPresentation(details: JsonValue): InputRequestPresentation {
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
    | unknown[]
    | undefined;
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
                    typeof choice.title === "string" ? choice.title : String(value),
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

function stringifyDetail(value: unknown, max = 280): string | null {
  if (value == null) return null;
  if (typeof value === "string") {
    const text = value.trim();
    if (!text) return null;
    return text.length > max ? `${text.slice(0, max)}…` : text;
  }
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  try {
    const text = JSON.stringify(value, null, 2);
    if (!text || text === "{}" || text === "[]") return null;
    return text.length > max ? `${text.slice(0, max)}…` : text;
  } catch {
    return null;
  }
}

function toolCallSummary(details: JsonValue): ToolCallSummary | null {
  const record = asRecord(details);
  if (!record) return null;

  const tool = asRecord(record.toolCall) ?? record;
  const title =
    [tool.title, tool.name, record.message, record.title]
      .find((value): value is string => typeof value === "string" && Boolean(value.trim())) ??
    "Agent action";

  const kind =
    typeof tool.kind === "string"
      ? tool.kind
      : typeof record.kind === "string"
        ? record.kind
        : undefined;
  const status = typeof tool.status === "string" ? tool.status : undefined;
  const toolCallId =
    typeof tool.toolCallId === "string"
      ? tool.toolCallId
      : typeof tool.id === "string"
        ? tool.id
        : undefined;

  const detailLines: string[] = [];
  const locations = Array.isArray(tool.locations) ? tool.locations : [];
  for (const location of locations.slice(0, 4)) {
    const loc = asRecord(location);
    if (!loc) continue;
    const path = typeof loc.path === "string" ? loc.path : null;
    if (!path) continue;
    const line = typeof loc.line === "number" ? `:${loc.line}` : "";
    detailLines.push(path + line);
  }

  const rawInput = stringifyDetail(tool.rawInput ?? tool.input ?? record.rawInput);
  if (rawInput) detailLines.push(rawInput);

  const description = stringifyDetail(tool.content ?? record.description ?? record.reason, 400);
  if (description && description !== title) detailLines.unshift(description);

  return { title, kind, status, toolCallId, detailLines };
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
        name: typeof item.name === "string" && item.name.trim() ? item.name : optionId,
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

function isAllowAlways(kind: string, optionId: string): boolean {
  return kind === "allow_always" || /always|session|remember/i.test(`${kind} ${optionId}`);
}

function isAllowKind(kind: string, optionId: string): boolean {
  return kind.startsWith("allow") || /allow|yes|approve/i.test(optionId);
}

function kindLabel(kind?: string): string | null {
  if (!kind) return null;
  return kind.replace(/[_-]+/g, " ");
}

function optionMeta(option: PermissionOption): {
  Icon: typeof ShieldCheck;
  tone: "allow" | "reject" | "neutral";
  hint: string;
} {
  if (isRejectKind(option.kind, option.optionId)) {
    return {
      Icon: ShieldX,
      tone: "reject",
      hint: option.kind === "reject_always" ? "Block and remember" : "Block this once",
    };
  }
  if (isAllowAlways(option.kind, option.optionId)) {
    return {
      Icon: ShieldCheck,
      tone: "allow",
      hint: "Allow and remember for this session",
    };
  }
  if (isAllowKind(option.kind, option.optionId)) {
    return {
      Icon: ShieldCheck,
      tone: "allow",
      hint: "Allow this action once",
    };
  }
  return {
    Icon: ShieldAlert,
    tone: "neutral",
    hint: "Choose this option",
  };
}

function ToolDetailCard({ tool }: { tool: ToolCallSummary }) {
  return (
    <div className="overflow-hidden rounded-lg border border-border/80 bg-muted/30">
      <div className="flex items-start gap-3 px-3 py-3">
        <div className="grid size-9 shrink-0 place-items-center rounded-md bg-background ring-1 ring-border">
          {tool.kind === "execute" ? (
            <Terminal className="size-4 text-foreground" />
          ) : tool.kind === "edit" || tool.kind === "write" ? (
            <FileCode2 className="size-4 text-foreground" />
          ) : (
            <Wrench className="size-4 text-foreground" />
          )}
        </div>
        <div className="min-w-0 flex-1 space-y-1.5">
          <div className="flex flex-wrap items-center gap-1.5">
            <p className="truncate text-sm font-medium text-foreground">{tool.title}</p>
            {tool.kind && (
              <Badge variant="outline" className="capitalize">
                {kindLabel(tool.kind)}
              </Badge>
            )}
            {tool.status && (
              <Badge variant="secondary" className="capitalize">
                {tool.status}
              </Badge>
            )}
          </div>
          {tool.toolCallId && (
            <p className="truncate font-mono text-[0.65rem] text-muted-foreground">
              {tool.toolCallId}
            </p>
          )}
        </div>
      </div>
      {tool.detailLines.length > 0 && (
        <>
          <Separator />
          <ScrollArea className="max-h-36">
            <div className="space-y-2 px-3 py-2.5">
              {tool.detailLines.map((line, index) => (
                <pre
                  key={`${index}-${line.slice(0, 24)}`}
                  className="whitespace-pre-wrap break-all font-mono text-[0.7rem] leading-relaxed text-muted-foreground"
                >
                  {line}
                </pre>
              ))}
            </div>
          </ScrollArea>
        </>
      )}
    </div>
  );
}

function PermissionOptionButton({
  option,
  disabled,
  onSelect,
}: {
  option: PermissionOption;
  disabled: boolean;
  onSelect: () => void;
}) {
  const { Icon, tone, hint } = optionMeta(option);
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        "group flex w-full items-start gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors",
        "disabled:pointer-events-none disabled:opacity-50",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40",
        tone === "allow" &&
          "border-border bg-background hover:border-primary/40 hover:bg-primary/5",
        tone === "reject" &&
          "border-border bg-background hover:border-destructive/40 hover:bg-destructive/5",
        tone === "neutral" &&
          "border-border bg-background hover:border-border hover:bg-muted/50",
      )}
    >
      <span
        className={cn(
          "mt-0.5 grid size-8 shrink-0 place-items-center rounded-md ring-1 ring-inset",
          tone === "allow" && "bg-primary/10 text-primary ring-primary/15",
          tone === "reject" && "bg-destructive/10 text-destructive ring-destructive/15",
          tone === "neutral" && "bg-muted text-muted-foreground ring-border",
        )}
      >
        <Icon className="size-3.5" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-medium text-foreground">{option.name}</span>
        <span className="mt-0.5 block text-[0.7rem] text-muted-foreground">{hint}</span>
      </span>
      <Check className="mt-1 size-3.5 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-60" />
    </button>
  );
}

export function PendingAgentRequestCard({
  request,
  onRespond,
}: {
  request: PendingAgentRequest;
  onRespond: (result: JsonValue) => Promise<void>;
}) {
  const isApproval = request.kind === "approval";
  const presentation = useMemo(
    () => (isApproval ? null : inputRequestPresentation(request.details)),
    [isApproval, request.details],
  );
  const options = useMemo(
    () => (isApproval ? permissionOptions(request.details) : []),
    [isApproval, request.details],
  );
  const tool = useMemo(() => toolCallSummary(request.details), [request.details]);

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

  const orderedOptions = useMemo(() => {
    if (options.length === 0) {
      return [
        { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
        { optionId: "reject-once", name: "Deny", kind: "reject_once" },
      ] satisfies PermissionOption[];
    }
    // Allow options first, then neutral, rejects last — safer default scan order.
    return [...options].sort((left, right) => {
      const rank = (option: PermissionOption) => {
        if (isRejectKind(option.kind, option.optionId)) return 2;
        if (isAllowKind(option.kind, option.optionId)) return 0;
        return 1;
      };
      return rank(left) - rank(right);
    });
  }, [options]);

  if (isApproval) {
    return (
      <Dialog open>
        <DialogContent
          showCloseButton={false}
          className="gap-0 overflow-hidden p-0 sm:max-w-md"
          onPointerDownOutside={(event) => event.preventDefault()}
          onEscapeKeyDown={(event) => event.preventDefault()}
        >
          <DialogHeader className="space-y-3 border-b border-border/70 px-5 py-4 text-left">
            <div className="flex items-start gap-3">
              <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-amber-500/10 text-amber-700 ring-1 ring-amber-500/20 dark:text-amber-300">
                <ShieldAlert className="size-4.5" />
              </div>
              <div className="min-w-0 flex-1 space-y-1">
                <div className="flex flex-wrap items-center gap-2">
                  <DialogTitle className="font-heading text-base">
                    Permission required
                  </DialogTitle>
                  <Badge variant="outline">Awaiting you</Badge>
                </div>
                <DialogDescription className="text-xs/relaxed">
                  The agent wants to run an action. Review it, then choose how to continue.
                </DialogDescription>
              </div>
            </div>
          </DialogHeader>

          <div className="space-y-3 px-5 py-4">
            {tool ? (
              <ToolDetailCard tool={tool} />
            ) : (
              <p className="text-sm text-muted-foreground">
                Review this request before the agent continues.
              </p>
            )}

            <div className="space-y-2">
              <p className="text-[0.7rem] font-medium tracking-wide text-muted-foreground uppercase">
                Choose an option
              </p>
              <div className="space-y-2">
                {orderedOptions.map((option) => (
                  <PermissionOptionButton
                    key={option.optionId}
                    option={option}
                    disabled={submitting}
                    onSelect={() => void submit(selectedPermissionResult(option.optionId))}
                  />
                ))}
              </div>
            </div>
          </div>

          {submitting && (
            <DialogFooter className="border-t border-border/70 px-5 py-3 sm:justify-start">
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <LoaderCircle className="size-3.5 animate-spin" />
                Sending decision to the agent…
              </div>
            </DialogFooter>
          )}
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
        className="gap-0 overflow-hidden p-0 sm:max-w-md"
        onPointerDownOutside={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => event.preventDefault()}
      >
        <DialogHeader className="space-y-3 border-b border-border/70 px-5 py-4 text-left">
          <div className="flex items-start gap-3">
            <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-primary/10 text-primary ring-1 ring-primary/15">
              <CircleHelp className="size-4.5" />
            </div>
            <div className="min-w-0 flex-1 space-y-1">
              <div className="flex flex-wrap items-center gap-2">
                <DialogTitle className="font-heading text-base">
                  Input needed
                </DialogTitle>
                <Badge variant="outline">Question</Badge>
              </div>
              <DialogDescription className="text-xs/relaxed whitespace-pre-wrap">
                {presentation?.message}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="space-y-3 px-5 py-4">
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
                  : String(field.choices.findIndex((item) => item.value === choice))
              }
              onValueChange={(value) =>
                setChoice(field.choices[Number(value)]?.value ?? null)
              }
              className="gap-2"
            >
              {field.choices.map((item, index) => {
                const id = `${request.requestId}-choice-${index}`;
                const selected = choice === item.value;
                return (
                  <label
                    key={`${item.label}-${String(item.value)}`}
                    htmlFor={id}
                    className={cn(
                      "flex cursor-pointer items-center gap-3 rounded-lg border px-3 py-2.5 text-sm transition-colors",
                      selected
                        ? "border-primary/40 bg-primary/5"
                        : "border-border bg-background hover:bg-muted/40",
                    )}
                  >
                    <RadioGroupItem id={id} value={String(index)} />
                    <span className="min-w-0 flex-1">{item.label}</span>
                  </label>
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

        <DialogFooter className="border-t border-border/70 px-5 py-3">
          <Button
            disabled={!canSubmit || submitting}
            className="min-w-24"
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
