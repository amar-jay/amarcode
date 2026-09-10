import { useMemo, useRef, useState } from "react";
import { ChevronRight, Search } from "lucide-react";
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

type DirectionFilter = "all" | RawActivityEvent["direction"];

const eventTimeFormatter = new Intl.DateTimeFormat(undefined, {
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
  fractionalSecondDigits: 3,
});

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
  loading = false,
  error = null,
}: {
  events: RawActivityEvent[];
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
                      className={cn("cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset text-xs",
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
                      <TableCell className={cn("align-top text-center font-medium",
												sent ? "text-green-700 dark:text-green-400" : "text-red-700 dark:text-red-400"
											)}>
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
