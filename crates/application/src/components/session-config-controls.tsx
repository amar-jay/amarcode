"use client";

import { useEffect, useRef, useState } from "react";
import NewChatOptionsMenu from "./new-chat-options-menu";
import { Check } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { PromptInputButton } from "@/components/ai-elements/prompt-input";
import type { SessionConfigOption, SessionConfigValue } from "@/types";
import { isRenderableConfigOption } from "@/state/session-config";

export function SessionConfigControls({
  options,
  disabled,
  onChange,
}: {
  options: SessionConfigOption[];
  disabled?: boolean;
  onChange: (configId: string, value: SessionConfigValue) => void;
}) {
  const visible = options.filter(isRenderableConfigOption);
  const containerRef = useRef<HTMLDivElement>(null);
  const controlsRef = useRef<HTMLDivElement>(null);
  const [collapsed, setCollapsed] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    const controls = controlsRef.current;
    const toolbar = container?.parentElement;
    if (!container || !controls || !toolbar) return;

    const measure = () => {
      const siblings = Array.from(toolbar.children).filter(
        (child) => child !== container,
      );
      const gap = Number.parseFloat(getComputedStyle(toolbar).columnGap) || 0;
      const requiredWidth =
        controls.getBoundingClientRect().width +
        siblings.reduce(
          (width, child) => width + child.getBoundingClientRect().width,
          0,
        ) +
        gap * siblings.length;
      setCollapsed(requiredWidth > toolbar.clientWidth);
    };
    const observer = new ResizeObserver(measure);
    const observeSizes = () => {
      observer.disconnect();
      observer.observe(toolbar);
      observer.observe(controls);
      for (const sibling of toolbar.children) observer.observe(sibling);
    };
    observeSizes();
    const mutations = new MutationObserver(observeSizes);
    mutations.observe(toolbar, { childList: true });
    return () => {
      observer.disconnect();
      mutations.disconnect();
    };
  }, [visible.length]);

  if (!visible.length) return null;

  return (
    <div ref={containerRef} className="relative flex shrink-0 items-center">
      <div
        className={
          collapsed ? "invisible absolute h-0 w-0 overflow-hidden" : undefined
        }
        inert={collapsed}
        aria-hidden={collapsed || undefined}
      >
        <div ref={controlsRef} className="flex w-max items-center gap-1">
          {visible.map((option) =>
            option.type === "boolean" ? (
              <PromptInputButton
                key={option.id}
                size="sm"
                disabled={disabled}
                aria-pressed={Boolean(option.current_value)}
                tooltip={option.description ?? option.name}
                onClick={() =>
                  onChange(option.id, {
                    type: "boolean",
                    value: !option.current_value,
                  })
                }
              >
                <span
                  aria-hidden="true"
                  data-state={option.current_value ? "checked" : "unchecked"}
                  className="group/switch inline-flex h-[14px] w-6 shrink-0 scale-75 items-center rounded-full border border-transparent bg-input transition-colors data-[state=checked]:bg-primary dark:bg-input/80"
                >
                  <span className="block size-3 rounded-full bg-background transition-transform group-data-[state=checked]/switch:translate-x-[calc(100%-2px)] dark:bg-foreground dark:group-data-[state=checked]/switch:bg-primary-foreground" />
                </span>
                <span>{option.name}</span>
              </PromptInputButton>
            ) : (
              <DropdownMenu key={option.id}>
                <DropdownMenuTrigger asChild>
                  <PromptInputButton
                    size="sm"
                    disabled={disabled}
                    tooltip={option.description ?? option.name}
                  >
                    <span>
                      {(option.options ?? []).find(
                        (choice) => choice.value === option.current_value,
                      )?.name ?? option.name}
                    </span>
                  </PromptInputButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="min-w-36">
                  {(option.options ?? []).map((choice) => (
                    <DropdownMenuItem
                      key={choice.value}
                      title={choice.description ?? undefined}
                      onSelect={() =>
                        onChange(option.id, { type: "id", value: choice.value })
                      }
                    >
                      <span className="min-w-0 flex-1">{choice.name}</span>
                      {option.current_value === choice.value && (
                        <Check className="ml-auto size-3.5" />
                      )}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            ),
          )}
        </div>
      </div>
      {collapsed && (
        <NewChatOptionsMenu
          options={visible}
          showPermissionFallback={false}
          disabled={disabled}
          onConfigChange={onChange}
        />
      )}
    </div>
  );
}
