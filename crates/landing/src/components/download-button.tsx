import { ArrowDownToLine, ArrowUpRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { RELEASES_URL, useDownload } from "@/hooks/use-download";

export default function DownloadButton() {
  const { href, label } = useDownload();
  return (
    <Button
      asChild
      className="h-auto min-h-12 leading-[1.428571] gap-[22px] rounded-none px-[23px] py-3 text-[19px] font-normal shadow-none transition-colors duration-200 motion-reduce:transition-none border border-ink bg-ink text-paper hover:bg-[#3d4944]"
    >
      <a href={href}>
        {label}
        {href === RELEASES_URL ? <ArrowUpRight /> : <ArrowDownToLine />}
      </a>
    </Button>
  );
}
