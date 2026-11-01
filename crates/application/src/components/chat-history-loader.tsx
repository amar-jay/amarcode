import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import { LoaderCircle } from "lucide-react";
import { useStickToBottomContext } from "use-stick-to-bottom";
import { Button } from "./ui/button";

/** Page upward without pulling the reader away from their current message. */
export function ChatHistoryLoader({
  oldestId,
  hasOlder,
  loading,
  error,
  onLoad,
}: {
  oldestId?: string;
  hasOlder: boolean;
  loading: boolean;
  error?: string | null;
  onLoad: () => Promise<boolean>;
}) {
  const { scrollRef, stopScroll } = useStickToBottomContext();
  const anchor = useRef<{
    oldestId?: string;
    height: number;
    top: number;
    node?: Element;
    offset?: number;
  } | null>(null);
  const load = useCallback(async () => {
    const scroll = scrollRef.current;
    if (!scroll || loading || !hasOlder || anchor.current) return;
    const node = [...scroll.querySelectorAll("[data-chat-search-key]")].find(
      (item) =>
        item.getBoundingClientRect().bottom >
        scroll.getBoundingClientRect().top,
    );
    anchor.current = {
      oldestId,
      height: scroll.scrollHeight,
      top: scroll.scrollTop,
      node,
      offset: node?.getBoundingClientRect().top,
    };
    stopScroll();
    if (!(await onLoad())) anchor.current = null;
  }, [scrollRef, stopScroll, oldestId, loading, hasOlder, onLoad]);

  useLayoutEffect(() => {
    const previous = anchor.current;
    const scroll = scrollRef.current;
    if (!previous || previous.oldestId === oldestId || !scroll) return;
    if (previous.node?.isConnected && previous.offset !== undefined) {
      scroll.scrollTop +=
        previous.node.getBoundingClientRect().top - previous.offset;
    } else {
      scroll.scrollTop = previous.top + scroll.scrollHeight - previous.height;
    }
    anchor.current = null;
  }, [oldestId, scrollRef]);

  useEffect(() => {
    if (!loading && anchor.current?.oldestId === oldestId)
      anchor.current = null;
  }, [loading, oldestId]);

  useEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll) return;
    let previousTop = scroll.scrollTop;
    const onScroll = () => {
      const top = scroll.scrollTop;
      if (anchor.current) {
        anchor.current.top = top;
        anchor.current.offset =
          anchor.current.node?.getBoundingClientRect().top;
      }
      if (top < previousTop && top < 160 && !error) void load();
      previousTop = top;
    };
    scroll.addEventListener("scroll", onScroll, { passive: true });
    return () => scroll.removeEventListener("scroll", onScroll);
  }, [scrollRef, load, error]);

  if (!hasOlder) return null;
  return (
    <div className="flex flex-col items-center gap-1 py-2">
      <Button
        variant="ghost"
        size="sm"
        disabled={loading}
        onClick={() => void load()}
      >
        {loading && (
          <LoaderCircle className="size-3 animate-spin motion-reduce:animate-none" />
        )}
        {loading
          ? "Loading earlier messages…"
          : error
            ? "Retry loading earlier messages"
            : "Load earlier messages"}
      </Button>
      {error && (
        <p
          role="alert"
          className="max-w-sm text-center text-xs text-destructive"
        >
          {error}
        </p>
      )}
    </div>
  );
}
