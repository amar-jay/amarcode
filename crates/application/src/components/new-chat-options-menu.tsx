import { PermissionMode, permissionModeAtom } from "@/state";
import { SessionConfigOption, SessionConfigValue } from "@/types";
import { Ellipsis, ShieldQuestion, ShieldCheck, Check } from "lucide-react";
import { PromptInputButton } from "./ai-elements/prompt-input";
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuLabel, DropdownMenuSub, DropdownMenuSubTrigger, DropdownMenuSubContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuCheckboxItem } from "./ui/dropdown-menu";
import { useAtom } from "jotai/react";

function NewChatOptionsMenu({
  options,
  showPermissionFallback,
  onConfigChange,
}: {
  options: SessionConfigOption[];
  showPermissionFallback: boolean;
  onConfigChange: (configId: string, value: SessionConfigValue) => void;
}) {
  const visibleOptions = options.filter(
    (option) => option.type === "boolean" || option.type === "select",
  );

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <PromptInputButton tooltip="More options" aria-label="More options">
          <Ellipsis className="size-4" />
        </PromptInputButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-44">
        {/* <DropdownMenuLabel>New chat options</DropdownMenuLabel> */}
        {showPermissionFallback && (<PermissionFallbackMenuSub visibleOptions={visibleOptions} />)}
        {visibleOptions.map((option) =>
          option.type === "boolean" ? (
            <DropdownMenuCheckboxItem
              key={option.id}
              checked={Boolean(option.current_value)}
              title={option.description ?? undefined}
              onCheckedChange={(checked) =>
                onConfigChange(option.id, {
                  type: "boolean",
                  value: checked === true,
                })
              }
            >
              {option.name}
            </DropdownMenuCheckboxItem>
          ) : (
            <DropdownMenuSub key={option.id}>
              <DropdownMenuSubTrigger title={option.description ?? undefined}>
                <span>{option.name}</span>
                <span className="ml-auto max-w-20 truncate text-[10px] text-muted-foreground">
                  {
                    (option.options ?? []).find(
                      (choice) => choice.value === option.current_value,
                    )?.name
                  }
                </span>
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-36">
                {(option.options ?? []).map((choice) => (
                  <DropdownMenuItem
                    key={choice.value}
                    title={choice.description ?? undefined}
                    onSelect={() =>
                      onConfigChange(option.id, {
                        type: "id",
                        value: choice.value,
                      })
                    }
                  >
                    <span className="min-w-0 flex-1">{choice.name}</span>
                    {option.current_value === choice.value && <Check />}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          ),
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

const permissionModes: Array<{
  value: PermissionMode;
  label: string;
}> = [
  {
    value: "confirm",
    label: "Confirm",
  },
  {
    value: "autoedit",
    label: "Autoedit",
  },
  {
    value: "full-agentic",
    label: "Agentic",
  },
];


function PermissionFallbackMenuSub({
	visibleOptions,
}:{
	visibleOptions: SessionConfigOption[];
}) {
  const [permissionMode, setPermissionMode] = useAtom(permissionModeAtom);
	return (
     <>
		 <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              {permissionMode === "confirm" ? (
                <ShieldQuestion />
              ) : (
                <ShieldCheck />
              )}
              <span>Permissions</span>
              <span className="ml-auto text-[10px] text-muted-foreground">
                {
                  permissionModes.find((item) => item.value === permissionMode)
                    ?.label
                }
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="min-w-32">
              {permissionModes.map((item) => (
                <DropdownMenuItem
                  key={item.value}
                  onSelect={() => setPermissionMode(item.value)}
                >
                  <span className="min-w-0 flex-1">{item.label}</span>
                  {permissionMode === item.value && <Check />}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        {visibleOptions.length > 0 && (
          <DropdownMenuSeparator />
        )}
				</>
	)
}
export default NewChatOptionsMenu;