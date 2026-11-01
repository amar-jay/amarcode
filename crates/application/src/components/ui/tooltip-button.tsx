import type { ComponentProps } from "react";
import { Button } from "./button";
import { Tooltip, TooltipContent, TooltipTrigger } from "./tooltip";

/** Toolbar button whose hover label uses the application's tooltip styling. */
export function TooltipButton({
  title,
  ...props
}: ComponentProps<typeof Button>) {
  const button = <Button {...props} />;
  if (!title) return button;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent side="bottom">{title}</TooltipContent>
    </Tooltip>
  );
}
