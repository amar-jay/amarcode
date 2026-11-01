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
  const { scrollRef, stopScroll, isAtBottom } = useStickToBottomContext();
  const sentinel = useRef<HTMLDivElement>(null);
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
    const target = sentinel.current;
    if (!scroll || !target || !hasOlder || loading || error) return;

    const observer = new IntersectionObserver(
      (entries) => {
        // Opening a chat starts at the newest message. Only page upward when
        // the history edge is visible, or the current page cannot fill the view.
        if (
          entries.some((entry) => entry.isIntersecting) &&
          (!isAtBottom || scroll.scrollHeight <= scroll.clientHeight)
        )
          void load();
      },
      { root: scroll, rootMargin: "160px 0px 0px 0px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [scrollRef, load, hasOlder, loading, error, isAtBottom]);

  useEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll) return;
    const onScroll = () => {
      if (anchor.current) {
        anchor.current.top = scroll.scrollTop;
        anchor.current.offset =
          anchor.current.node?.getBoundingClientRect().top;
      }
    };
    scroll.addEventListener("scroll", onScroll, { passive: true });
    return () => scroll.removeEventListener("scroll", onScroll);
  }, [scrollRef]);

  if (!hasOlder) return null;
  return (
    <div
      ref={sentinel}
      className="flex min-h-8 flex-col items-center justify-center gap-1 py-2"
    >
      {loading && (
        <span
          role="status"
          className="flex items-center gap-2 text-xs text-muted-foreground"
        >
          <LoaderCircle className="size-3 animate-spin motion-reduce:animate-none" />
          Loading earlier messages…
        </span>
      )}
      {error && (
        <Button variant="ghost" size="sm" onClick={() => void load()}>
          Retry loading earlier messages
        </Button>
      )}
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
