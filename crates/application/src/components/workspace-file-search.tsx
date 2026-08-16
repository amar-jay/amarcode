import { useEffect, useRef, useState } from "react";
import { Search, X } from "lucide-react";
import { cn } from "@/lib/utils";

type WorkspaceFileSearchProps = {
  active: boolean;
  disabled: boolean;
  value: string;
  onValueChange: (value: string) => void;
  className?: string;
};

export function WorkspaceFileSearch({
  active,
  disabled,
  value,
  onValueChange,
  className,
}: WorkspaceFileSearchProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [isFocused, setIsFocused] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) inputRef.current?.focus();
  }, [isOpen]);

  useEffect(() => {
    if (!active || disabled) {
      setIsOpen(false);
      setIsFocused(false);
      onValueChange("");
    }
  }, [active, disabled, onValueChange]);

  useEffect(() => {
    if (!value && !isFocused) setIsOpen(false);
  }, [isFocused, value]);

  const closeSearch = () => {
    setIsOpen(false);
    setIsFocused(false);
    onValueChange("");
  };

  return (
    <div
      onFocus={() => setIsFocused(true)}
      onBlur={(event) => {
        const nextFocus = event.relatedTarget;
        if (
          !(nextFocus instanceof Node) ||
          !event.currentTarget.contains(nextFocus)
        ) {
          setIsFocused(false);
          if (!value) setIsOpen(false);
        }
      }}
      className={cn(
        "h-full shrink-0 ml-auto overflow-hidden transition-[width] duration-200 ease-out motion-reduce:transition-none",
        isOpen
          ? "w-[calc(100%-4.5rem)] opacity-100! backdrop-blur-none! h-5"
          : "w-fit",
        className,
      )}
    >
      <button
        type="button"
        disabled={disabled}
        aria-label="Search workspace files"
        title="Search workspace files"
        onClick={() => setIsOpen(true)}
        className={`inset-0 right-0 flex size-7 items-center justify-center rounded-md text-muted-foreground outline-none transition-[opacity,transform,color,background-color] hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 motion-reduce:transition-none ${
          isOpen
            ? "pointer-events-none translate-x-1 opacity-0"
            : "translate-x-0 opacity-100"
        }`}
      >
        <Search className="size-4 text-foreground" />
      </button>
      <div
        className={`absolute inset-0 flex origin-right items-center overflow-hidden rounded-md bg-muted/70 text-muted-foreground ring-0 transition-opacity duration-200 focus-within:bg-background focus-within:text-foreground focus-within:ring-0 motion-reduce:transition-none ${
          isOpen
            ? "opacity-100 bg-primary-foreground"
            : "pointer-events-none opacity-0"
        }`}
      >
        <Search aria-hidden="true" className="ml-2 size-3.5 shrink-0" />
        <input
          ref={inputRef}
          type="search"
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") closeSearch();
          }}
          aria-label="Search workspace files"
          placeholder="Search files"
          tabIndex={isOpen ? 0 : -1}
          className="min-w-0 flex-1 bg-transparent px-2 text-xs text-foreground outline-none placeholder:text-muted-foreground [&::-webkit-search-cancel-button]:hidden"
        />
        <button
          type="button"
          aria-label="Close file search"
          title="Close search"
          onClick={closeSearch}
          tabIndex={isOpen ? 0 : -1}
          className="mr-0.5 flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X className="size-3.5" />
        </button>
      </div>
    </div>
  );
}
