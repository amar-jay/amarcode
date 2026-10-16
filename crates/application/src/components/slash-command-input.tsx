import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useId, useLayoutEffect, useRef, useState } from "react";
import type { AvailableCommand } from "@/types";
import {
  commandInputHint,
  filterSlashCommands,
  slashCommandQuery,
} from "@/lib/slash-commands";
import {
  PromptInputTextarea,
  usePromptInputController,
} from "./ai-elements/prompt-input";

export function SlashCommandInput({
  commands,
  sessionReady,
  disabled,
  onDiscover,
  discovering = false,
  discoveryError,
  onRetry,
}: {
  commands: AvailableCommand[];
  sessionReady: boolean;
  disabled: boolean;
  onDiscover?: () => void;
  discovering?: boolean;
  discoveryError?: string | null;
  onRetry?: () => void;
}) {
  const { textInput } = usePromptInputController();
  const textarea = useRef<HTMLTextAreaElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const highlight = useRef<HTMLDivElement>(null);
  const id = useId();
  const [cursor, setCursor] = useState(0);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [activeName, setActiveName] = useState<string | null>(null);
  const query = slashCommandQuery(textInput.value, cursor);
  const matches = query === null ? [] : filterSlashCommands(commands, query);
  const activeIndex = Math.max(
    0,
    matches.findIndex((command) => command.name === activeName),
  );
  const visible =
    focused && !disabled && query !== null && dismissed !== textInput.value;
  const hint = commandInputHint(textInput.value, commands);
  const commandName = /^\/([^\s]+)/.exec(textInput.value)?.[1];
  const command = commands.find((candidate) => candidate.name === commandName);
  const commandToken = command ? `/${command.name}` : null;

  useLayoutEffect(() => {
    if (highlight.current && textarea.current) {
      highlight.current.scrollTop = textarea.current.scrollTop;
      highlight.current.scrollLeft = textarea.current.scrollLeft;
    }
  }, [commandToken, textInput.value]);

  useLayoutEffect(() => {
    if (visible)
      list.current
        ?.querySelector('[aria-selected="true"]')
        ?.scrollIntoView({ block: "nearest" });
  }, [visible, activeIndex, commands]);

  function choose(command: AvailableCommand) {
    const value = `/${command.name} `;
    textInput.setInput(value);
    setCursor(value.length);
    setActiveName(null);
    textarea.current?.focus();
    requestAnimationFrame(() =>
      textarea.current?.setSelectionRange(value.length, value.length),
    );
  }

  return (
    <div className="relative w-full min-w-0">
      {visible && (
        <div className="bg-popover max-w-md text-popover-foreground absolute inset-x-2 bottom-full z-50 mb-2 rounded-lg border shadow-lg">
          <div
            ref={list}
            id={id}
            role="listbox"
            aria-label="Agent slash commands"
            className="max-h-64 overflow-y-auto p-1"
          >
            {matches.map((command, index) => (
              <Tooltip key={command.name} open={index === activeIndex}>
                <TooltipTrigger asChild>
                  <div
                    id={`${id}-${index}`}
                    role="option"
                    aria-selected={index === activeIndex}
                    className={`cursor-pointer rounded-md px-3 py-1 text-sm ${index === activeIndex ? "bg-accent text-accent-foreground" : "hover:bg-accent/60"}`}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseMove={() => setActiveName(command.name)}
                    onClick={() => choose(command)}
                  >
                    <div className="flex flex-wrap items-baseline gap-x-2">
                      <span className="font-medium">/{command.name}</span>
                      {command.input?.hint && (
                        <span className="text-muted-foreground text-xs">
                          {command.input.hint}
                        </span>
                      )}
                    </div>
                  </div>
                </TooltipTrigger>
                <TooltipContent
                  side="right"
                  sideOffset={8}
                  className="break-words"
                >
                  {command.description}
                </TooltipContent>
              </Tooltip>
            ))}
            {matches.length === 0 && (
              <div
                role="status"
                className="text-muted-foreground px-3 py-0 text-sm"
              >
                {discoveryError ? (
                  <div className="space-y-2">
                    <p>{discoveryError}</p>
                    <button
                      type="button"
                      className="text-foreground underline"
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={onRetry}
                    >
                      Retry discovery
                    </button>
                  </div>
                ) : discovering ? (
                  "Discovering agent commands…"
                ) : !sessionReady ? (
                  "Start this agent’s session to discover its commands."
                ) : commands.length ? (
                  "No matching commands."
                ) : (
                  "This agent hasn’t advertised any commands."
                )}
              </div>
            )}
          </div>
        </div>
      )}
      <div className="relative">
        {commandToken && (
          <div
            ref={highlight}
            aria-hidden="true"
            className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre-wrap break-words px-2 py-2 text-sm md:text-xs/relaxed"
          >
            <mark className="rounded-sm bg-primary/15 text-primary ring-1 ring-primary/25">
              {commandToken}
            </mark>
            {textInput.value.slice(commandToken.length)}
            {"\u200b"}
          </div>
        )}
        <PromptInputTextarea
          ref={textarea}
          className={
            commandToken
              ? "relative text-transparent caret-foreground selection:bg-primary/20 selection:text-transparent"
              : undefined
          }
          onScroll={(event) => {
            if (highlight.current) {
              highlight.current.scrollTop = event.currentTarget.scrollTop;
              highlight.current.scrollLeft = event.currentTarget.scrollLeft;
            }
          }}
          aria-autocomplete="list"
          aria-controls={visible ? id : undefined}
          aria-expanded={visible}
          aria-activedescendant={
            visible && matches.length ? `${id}-${activeIndex}` : undefined
          }
          aria-describedby={hint ? `${id}-hint` : undefined}
          onFocus={() => {
            setFocused(true);
            if (!disabled && textInput.value.startsWith("/")) onDiscover?.();
          }}
          onBlur={() => setFocused(false)}
          onSelect={(event) => setCursor(event.currentTarget.selectionStart)}
          onChange={(event) => {
            setCursor(event.currentTarget.selectionStart);
            setActiveName(null);
            setDismissed(null);
            if (!disabled && /^\/\S*$/.test(event.currentTarget.value))
              onDiscover?.();
          }}
          onKeyDown={(event) => {
            if (
              !visible ||
              event.nativeEvent.isComposing ||
              event.shiftKey ||
              event.ctrlKey ||
              event.metaKey ||
              event.altKey
            )
              return;
            if (event.key === "Escape") {
              event.preventDefault();
              setDismissed(textInput.value);
            } else if (
              (event.key === "ArrowDown" || event.key === "ArrowUp") &&
              matches.length
            ) {
              event.preventDefault();
              const next =
                (activeIndex +
                  (event.key === "ArrowDown" ? 1 : -1) +
                  matches.length) %
                matches.length;
              setActiveName(matches[next].name);
            } else if (
              (event.key === "Enter" || event.key === "Tab") &&
              matches.length
            ) {
              event.preventDefault();
              choose(matches[activeIndex]);
            } else if (event.key === "Enter" && query === "") {
              event.preventDefault();
            }
          }}
        />
      </div>
      {hint && (
        <div
          id={`${id}-hint`}
          className="text-muted-foreground px-3 pb-2 text-xs"
        >
          {hint}
        </div>
      )}
    </div>
  );
}
