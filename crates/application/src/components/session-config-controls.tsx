"use client";

import { Check } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { PromptInputButton } from "@/components/ai-elements/prompt-input";
import { Switch } from "@/components/ui/switch";
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
  if (!visible.length) return null;

  return (
    <>
      {visible.map((option) =>
        option.type === "boolean" ? (
          <PromptInputButton
            key={option.id}
            size="sm"
            disabled={disabled}
            tooltip={option.description ?? option.name}
            onClick={() =>
              onChange(option.id, {
                type: "boolean",
                value: !option.current_value,
              })
            }
          >
            <Switch
              checked={Boolean(option.current_value)}
              className="pointer-events-none scale-75"
            />
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
    </>
  );
}
