import { ArrowDownToLine, ArrowUpRight, ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { RELEASES_URL, useDownload } from "@/hooks/use-download";

const platforms = ["Windows", "Linux", "macOS"] as const;

export default function DownloadButton() {
  const { href, label, installers, loading } = useDownload();
  return (
    <div className="inline-flex items-stretch justify-center">
      <Button
        asChild
        className="h-auto min-h-12 gap-[22px] rounded-none border border-ink bg-ink px-[23px] py-3 text-[19px] leading-[1.428571] font-normal text-paper shadow-none transition-colors duration-200 hover:bg-[#3d4944] motion-reduce:transition-none max-[601px]:gap-3 max-[601px]:px-4"
      >
        <a href={href}>
          {label}
          {href === RELEASES_URL ? <ArrowUpRight /> : <ArrowDownToLine />}
        </a>
      </Button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            aria-label="Other platforms"
            title="Other platforms"
            className="h-auto min-h-12 w-11 shrink-0 rounded-none border border-l-0 border-ink bg-ink p-0 font-normal text-paper shadow-none ring-inset transition-colors hover:bg-[#3d4944] focus-visible:outline-2 focus-visible:-outline-offset-4 focus-visible:outline-paper motion-reduce:transition-none"
          >
            <ChevronDown aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="start"
          sideOffset={8}
          className="w-64 rounded-none border border-rule bg-paper p-1 text-ink shadow-lg motion-reduce:animate-none [&_[data-slot=dropdown-menu-item]]:rounded-none"
        >
          <DropdownMenuLabel className="px-3 py-2 text-[18px] font-normal">
            Other platforms
          </DropdownMenuLabel>
          <DropdownMenuSeparator className="bg-rule" />
          {loading ? (
            <DropdownMenuItem disabled className="px-3 py-2 text-[16px]">
              Loading downloads…
            </DropdownMenuItem>
          ) : (
            platforms.map((os) => {
              const options = installers.filter(
                (installer) => installer.os === os,
              );
              return (
                <DropdownMenuGroup key={os}>
                  <DropdownMenuLabel className="px-3 pt-3 pb-1 text-[16px] font-normal text-muted-ink">
                    {os}
                  </DropdownMenuLabel>
                  {options.length ? (
                    options.map((installer) => (
                      <DropdownMenuItem
                        asChild
                        key={installer.href}
                        className="cursor-pointer px-3 py-2 text-[17px] focus:bg-panel"
                      >
                        <a href={installer.href}>
                          <ArrowDownToLine aria-hidden="true" />
                          {installer.label}
                        </a>
                      </DropdownMenuItem>
                    ))
                  ) : (
                    <DropdownMenuItem
                      disabled
                      className="px-3 py-2 text-[15px]"
                    >
                      No installer in the latest release
                    </DropdownMenuItem>
                  )}
                </DropdownMenuGroup>
              );
            })
          )}
          <DropdownMenuSeparator className="bg-rule" />
          <DropdownMenuItem
            asChild
            className="cursor-pointer px-3 py-2 text-[17px] focus:bg-panel"
          >
            <a href={RELEASES_URL}>
              All releases{" "}
              <ArrowUpRight className="ml-auto" aria-hidden="true" />
            </a>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
