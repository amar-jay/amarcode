import { useMemo, useRef, useState } from "react";
import { ChevronRight, Search } from "lucide-react";
import type { AgentRun } from "@/types";
import type { RawActivityEvent } from "@/lib/activity-types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { buildAgentRunTimeline } from "@/lib/agent-run-timeline";

type DirectionFilter = "all" | RawActivityEvent["direction"];

const eventTimeFormatter = new Intl.DateTimeFormat(undefined, {
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
  fractionalSecondDigits: 3,
});

const runColors = [
  "bg-emerald-500",
  "bg-violet-400",
  "bg-orange-400",
  "bg-sky-500",
  "bg-pink-400",
] as const;

function durationLabel(milliseconds: number) {
  const seconds = milliseconds / 1_000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  const minutes = seconds / 60;
  if (minutes < 60) return `${minutes.toFixed(minutes < 10 ? 1 : 0)}m`;
  const hours = minutes / 60;
  return `${hours.toFixed(hours < 10 ? 1 : 0)}h`;
}

function agentColor(agentId: string) {
  let hash = 0;
  for (const character of agentId)
    hash = (hash * 31 + character.charCodeAt(0)) | 0;
  return runColors[Math.abs(hash) % runColors.length];
}

function AgentRunTimeline({ runs }: { runs: AgentRun[] }) {
  const [capturedNow] = useState(() => Date.now());
  const timeline = useMemo(
    () => buildAgentRunTimeline(runs, capturedNow),
    [capturedNow, runs],
  );

  if (!timeline) return null;

  return (
    <section
      className="shrink-0 border-b bg-muted/20"
      aria-label="Agent run timeline"
    >
      <div className="flex items-center justify-between border-b px-6 py-1.5 text-[10px] text-muted-foreground">
        <span className="font-medium uppercase tracking-wider">Agent runs</span>
        <span className="font-mono tabular-nums">
          {runs.length} runs · {durationLabel(timeline.activeDuration)} active
          {timeline.elapsedDuration > timeline.activeDuration * 2 && (
            <> across {durationLabel(timeline.elapsedDuration)}</>
          )}
        </span>
      </div>
      <div className="max-h-28 overflow-y-auto px-1">
        {timeline.lanes.map(([agentId, lane]) => (
          <div key={agentId} className="flex max-h-5 items-center gap-2">
            <div
              className="w-20 shrink-0 truncate text-right font-mono text-[9px] text-muted-foreground"
              title={agentId}
            >
              {agentId}
            </div>
            <div className="relative h-5 flex-1 border-b-1 overflow-hidden bg-muted/60">
              {[25, 50, 75].map((position) => (
                <span
                  key={position}
                  className="absolute inset-y-0 border-l border-background/70"
                  style={{ left: `${position}%` }}
                />
              ))}
              {timeline.breaks.map((gap) => (
                <span
                  key={`${agentId}-${gap.position}`}
                  className="absolute inset-y-0 z-10 w-2 -translate-x-1 bg-muted/60"
                  style={{ left: `${gap.position}%` }}
                  title={`${durationLabel(gap.duration)} inactive`}
                  aria-label={`${durationLabel(gap.duration)} inactive gap`}
                >
                  <span className="absolute inset-y-0 left-0.5 -skew-x-12 border-l border-foreground/35" />
                  <span className="absolute inset-y-0 right-0.5 -skew-x-12 border-l border-foreground/35" />
                </span>
              ))}
              {lane.map(({ run, start, end, visualStart, visualEnd }) => {
                const left = (visualStart / timeline.visualSpan) * 100;
                const width =
                  ((visualEnd - visualStart) / timeline.visualSpan) * 100;
                const stoppedAt = run.finished_at ?? "Still running";
                const title = `${run.agent_id}\n${run.status}\n${new Date(start).toLocaleString()} → ${stoppedAt === "Still running" ? stoppedAt : new Date(end).toLocaleString()}\n${durationLabel(Math.max(end - start, 0))}`;
                return (
                  <span
                    key={run.id}
                    className={cn(
                      "absolute inset-y-0 rounded-[2px] ring-1 ring-black/10",
                      agentColor(run.agent_id),
                      run.status === "failed" && "bg-red-400",
                      run.status === "running" && "animate-pulse",
                    )}
                    style={{ left: `${left}%`, width: `max(3px, ${width}%)` }}
                    title={title}
                    aria-label={title.replaceAll("\n", ", ")}
                  />
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

function ActivitySearch({ onSearch }: { onSearch: (query: string) => void }) {
  const [draftQuery, setDraftQuery] = useState("");

  return (
    <form
      className="flex min-w-0 flex-1 gap-2 sm:max-w-md"
      onSubmit={(event) => {
        event.preventDefault();
        onSearch(draftQuery);
      }}
    >
      <div className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <Input
          type="search"
          value={draftQuery}
          onChange={(event) => setDraftQuery(event.target.value)}
          placeholder="Search activity"
          aria-label="Search activity"
          className="pl-7 [&::-webkit-search-cancel-button]:hidden"
        />
      </div>
      <Button type="submit" size="sm" variant="secondary">
        Search
      </Button>
    </form>
  );
}

function primitiveValue(value: unknown) {
  if (typeof value === "string") {
    return (
      <span className="text-emerald-700 dark:text-emerald-400">
        {JSON.stringify(value)}
      </span>
    );
  }
  if (typeof value === "number") {
    return <span className="text-sky-700 dark:text-sky-400">{value}</span>;
  }
  if (typeof value === "boolean") {
    return (
      <span className="text-amber-700 dark:text-amber-400">
        {String(value)}
      </span>
    );
  }
  return <span className="text-muted-foreground">null</span>;
}

function JsonTreeNode({
  name,
  value,
  depth = 0,
  defaultOpen = false,
}: {
  name?: string;
  value: unknown;
  depth?: number;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const isObject = typeof value === "object" && value !== null;
  if (!isObject) {
    return (
      <div
        className="flex min-w-0 gap-2 py-0.5"
        style={{ paddingLeft: depth * 14 }}
      >
        {name !== undefined && (
          <span className="shrink-0 text-foreground">{name}:</span>
        )}
        <span className="min-w-0 break-all">{primitiveValue(value)}</span>
      </div>
    );
  }

  const entries = Object.entries(value);
  const array = Array.isArray(value);
  const summary = `${array ? "Array" : "Object"}(${entries.length})`;

  return (
    <div>
      <button
        type="button"
        className="flex w-full items-center py-0.5 text-left text-foreground hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        style={{ paddingLeft: depth * 14 }}
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <ChevronRight
          className={`mr-1 size-3 shrink-0 text-muted-foreground transition-transform ${open ? "rotate-90" : ""}`}
          aria-hidden="true"
        />
        {name !== undefined && <span>{name}: </span>}
        <span className="ml-1 text-muted-foreground">{summary}</span>
      </button>
      {open &&
        entries.map(([key, child]) => (
          <JsonTreeNode key={key} name={key} value={child} depth={depth + 1} />
        ))}
    </div>
  );
}

export function ActivityView({
  events,
  runs,
  loading = false,
  error = null,
}: {
  events: RawActivityEvent[];
  runs: AgentRun[];
  loading?: boolean;
  error?: string | null;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [direction, setDirection] = useState<DirectionFilter>("all");
  const [selectedEventId, setSelectedEventId] = useState<number | null>(null);
  const normalizedQuery = query.trim().toLocaleLowerCase();

  const visibleEvents = useMemo(
    () =>
      events.filter((event) => {
        if (direction !== "all" && event.direction !== direction) return false;
        if (!normalizedQuery) return true;
        return `${event.method}\n${JSON.stringify(event.payload)}`
          .toLocaleLowerCase()
          .includes(normalizedQuery);
      }),
    [direction, events, normalizedQuery],
  );
  const selectedEvent =
    events.find((event) => event.id === selectedEventId) ?? null;

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label="Activity">
      <div className="flex min-h-11 items-center gap-2 border-b px-6 py-2">
        <ActivitySearch onSearch={setQuery} />
        <Select
          value={direction}
          onValueChange={(value) => setDirection(value as DirectionFilter)}
        >
          <SelectTrigger size="sm" aria-label="Filter activity by direction">
            <SelectValue />
          </SelectTrigger>
          <SelectContent align="end">
            <SelectItem value="all">All events</SelectItem>
            <SelectItem value="sent">Sent</SelectItem>
            <SelectItem value="received">Received</SelectItem>
          </SelectContent>
        </Select>
        <span className="hidden text-[10px] tabular-nums text-muted-foreground sm:inline">
          {visibleEvents.length} of {events.length}
        </span>
      </div>

      <AgentRunTimeline runs={runs} />

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <div
          ref={scrollRef}
          className="min-h-0 w-[52%] min-w-96 shrink-0 overflow-y-auto border-r"
        >
          <div className="w-full px-6 py-4">
            <Table className="table-fixed">
              <TableHeader className="sticky top-0 z-10 bg-background">
                <TableRow className="hover:bg-transparent">
                  <TableHead className="w-20 text-muted-foreground">
                    Event
                  </TableHead>
                  <TableHead className="w-32 text-muted-foreground">
                    Time
                  </TableHead>
                  <TableHead className="w-20 text-center text-xs text-muted-foreground">
                    Flow
                  </TableHead>
                  <TableHead className="text-xs text-muted-foreground">
                    Method
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visibleEvents.map((event) => {
                  const sent = event.direction === "sent";
                  return (
                    <TableRow
                      key={event.id}
                      data-state={
                        selectedEventId === event.id ? "selected" : undefined
                      }
                      tabIndex={0}
                      className={cn(
                        "cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset text-xs",
                        // sent ? "bg-emerald-100/50 dark:bg-emerald-900" : "bg-orange-100/50 dark:bg-orange-900"
                      )}
                      onClick={() => setSelectedEventId(event.id)}
                      onKeyDown={(keyboardEvent) => {
                        if (
                          keyboardEvent.key === "Enter" ||
                          keyboardEvent.key === " "
                        ) {
                          keyboardEvent.preventDefault();
                          setSelectedEventId(event.id);
                        }
                      }}
                    >
                      <TableCell className="align-top text-xs tabular-nums">
                        #{event.id}
                      </TableCell>
                      <TableCell className="align-top font-mono text-xs tabular-nums">
                        <time
                          dateTime={event.createdAt}
                          title={new Date(event.createdAt).toLocaleString()}
                        >
                          {eventTimeFormatter.format(new Date(event.createdAt))}
                        </time>
                      </TableCell>
                      <TableCell
                        className={cn(
                          "align-top text-center font-medium",
                          sent
                            ? "text-green-700 dark:text-green-400"
                            : "text-red-700 dark:text-red-400",
                        )}
                      >
                        {sent ? "Sent" : "Recv"}
                      </TableCell>
                      <TableCell className="overflow-hidden text-ellipsis align-top">
                        {event.method}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </div>
          {loading && (
            <div className="flex min-h-48 items-center justify-center px-6 text-sm text-muted-foreground">
              Loading activity…
            </div>
          )}
          {!loading && error && (
            <div className="flex min-h-48 items-center justify-center px-6 text-sm text-destructive">
              Failed to load activity: {error}
            </div>
          )}
          {!loading && !error && !visibleEvents.length && (
            <div className="flex min-h-48 items-center justify-center px-6 text-sm text-muted-foreground">
              No activity matches the current filters.
            </div>
          )}
        </div>

        <aside
          className="min-w-0 flex-1 overflow-auto"
          aria-label="Event payload"
        >
          {selectedEvent ? (
            <div className="min-w-max p-5 font-mono text-xs leading-5">
              <div className="mb-4 border-b pb-3">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                  Event #{selectedEvent.id}
                </div>
                <div className="mt-1 font-sans text-sm font-medium text-foreground">
                  {selectedEvent.method}
                </div>
                <div className="mt-1 text-[10px] text-muted-foreground">
                  Run {selectedEvent.agentRunId}
                </div>
              </div>
              <JsonTreeNode
                key={selectedEvent.id}
                value={selectedEvent.payload}
                defaultOpen
              />
            </div>
          ) : (
            <div className="flex min-h-48 items-center justify-center px-6 text-sm text-muted-foreground">
              Select an event to inspect its payload.
            </div>
          )}
        </aside>
      </div>
    </section>
  );
}
