import { useEffect, useState } from "react"
import { Check, Cog, Monitor, Moon, Palette, RotateCcw, SlidersHorizontal, Sun } from "lucide-react"
import type { Theme } from "@/hooks/use-theme"
import { Breadcrumb, BreadcrumbItem, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/components/ui/breadcrumb"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { Separator } from "@/components/ui/separator"
import { Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarMenu, SidebarMenuButton, SidebarMenuItem, SidebarProvider } from "@/components/ui/sidebar"
import { Switch } from "@/components/ui/switch"

type SettingsPage = "appearance" | "general"

const navigation: { id: SettingsPage; label: string; icon: typeof Palette }[] = [
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "general", label: "General", icon: SlidersHorizontal },
]

const themeChoices: { value: Theme; label: string; description: string; Icon: typeof Sun }[] = [
  { value: "light", label: "Light", description: "Warm and clear", Icon: Sun },
  { value: "dark", label: "Dark", description: "Soft low-light", Icon: Moon },
  { value: "system", label: "System", description: "Match your device", Icon: Monitor },
]

function usePreference(key: string, defaultValue: boolean) {
  const [value, setValue] = useState(() => localStorage.getItem(key) === null ? defaultValue : localStorage.getItem(key) === "true")
  useEffect(() => localStorage.setItem(key, String(value)), [key, value])
  return [value, setValue] as const
}

export function SettingsDialog({ open, onOpenChange, theme, onThemeChange }: { open: boolean; onOpenChange: (open: boolean) => void; theme: Theme; onThemeChange: (theme: Theme) => void }) {
  const [page, setPage] = useState<SettingsPage>("appearance")
  const [restoreWorkspace, setRestoreWorkspace] = usePreference("acp-workbench-restore-workspace", true)
  const [timestamps, setTimestamps] = usePreference("acp-workbench-show-timestamps", false)

  return <Dialog open={open} onOpenChange={onOpenChange}>
    <DialogContent className="!h-[500px] !w-[800px] !max-w-[800px] overflow-hidden p-0">
      <DialogTitle className="sr-only">Settings</DialogTitle>
      <DialogDescription className="sr-only">Customize your ACP Workbench preferences.</DialogDescription>
      <SidebarProvider className="h-full min-h-0 items-start" style={{ "--sidebar-width": "12.5rem" } as React.CSSProperties}>
        <Sidebar collapsible="none" className="hidden border-r border-border bg-muted/25 md:flex">
          <div className="flex h-16 items-center gap-2 px-5"><Cog className="size-4 text-muted-foreground" /><span className="text-sm font-medium">Settings</span></div>
          <SidebarContent className="px-3"><SidebarGroup className="p-0"><SidebarGroupContent><SidebarMenu>{navigation.map(({ id, label, icon: Icon }) => <SidebarMenuItem key={id}><SidebarMenuButton isActive={page === id} onClick={() => setPage(id)}><Icon /><span>{label}</span></SidebarMenuButton></SidebarMenuItem>)}</SidebarMenu></SidebarGroupContent></SidebarGroup></SidebarContent>
        </Sidebar>
        <main className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-popover">
          <header className="flex h-16 shrink-0 items-center px-6"><Breadcrumb><BreadcrumbList><BreadcrumbItem className="hidden md:block"><span className="text-muted-foreground">Settings</span></BreadcrumbItem><BreadcrumbSeparator className="hidden md:block" /><BreadcrumbItem><BreadcrumbPage>{navigation.find((item) => item.id === page)?.label}</BreadcrumbPage></BreadcrumbItem></BreadcrumbList></Breadcrumb></header>
          <div className="flex flex-1 flex-col overflow-y-auto p-6">{page === "appearance" ? <AppearancePanel theme={theme} onThemeChange={onThemeChange} /> : <GeneralPanel restoreWorkspace={restoreWorkspace} setRestoreWorkspace={setRestoreWorkspace} timestamps={timestamps} setTimestamps={setTimestamps} />}</div>
        </main>
      </SidebarProvider>
    </DialogContent>
  </Dialog>
}

function AppearancePanel({ theme, onThemeChange }: { theme: Theme; onThemeChange: (theme: Theme) => void }) {
  return <div className="mx-auto w-full max-w-[33rem]"><div><p className="text-sm font-medium">Theme</p><p className="mt-1 text-xs leading-5 text-muted-foreground">Choose a theme or follow the operating system preference.</p></div><RadioGroup value={theme} onValueChange={(value) => onThemeChange(value as Theme)} className="mt-5 grid grid-cols-3 gap-3">{themeChoices.map(({ value, label, description, Icon }) => <label htmlFor={`theme-${value}`} key={value} data-active={theme === value || undefined} className="group relative cursor-pointer rounded-xl border border-border p-3 transition-all hover:-translate-y-0.5 hover:shadow-sm data-active:border-primary data-active:ring-2 data-active:ring-primary/20"><ThemePreview theme={value} /><div className="mt-3 flex items-start gap-2"><RadioGroupItem id={`theme-${value}`} value={value} className="mt-0.5" /><span><span className="flex items-center gap-1 text-xs font-medium"><Icon className="size-3" />{label}</span><span className="mt-0.5 block text-[10px] leading-4 text-muted-foreground">{description}</span></span></div>{theme === value && <span className="absolute right-3 top-3 grid size-4 place-items-center rounded-full bg-primary text-primary-foreground"><Check className="size-2.5" /></span>}</label>)}</RadioGroup><p className="mt-6 rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-[11px] leading-4 text-muted-foreground">System uses your device preference and updates automatically when it changes.</p></div>
}

function GeneralPanel({ restoreWorkspace, setRestoreWorkspace, timestamps, setTimestamps }: { restoreWorkspace: boolean; setRestoreWorkspace: (value: boolean) => void; timestamps: boolean; setTimestamps: (value: boolean) => void }) {
  return <div className="mx-auto w-full max-w-[33rem]"><h2 className="text-base font-medium">General</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">Set the defaults for starting and reviewing agent sessions.</p><Separator className="my-6" /><div className="divide-y divide-border rounded-xl border border-border"><PreferenceRow title="Restore recent workspace" description="Preselect the most recently used project when creating a session." checked={restoreWorkspace} onCheckedChange={setRestoreWorkspace} /><PreferenceRow title="Show activity timestamps" description="Display local timestamps beside streamed agent activity." checked={timestamps} onCheckedChange={setTimestamps} /></div><div className="mt-6 flex items-center justify-between"><p className="text-[11px] text-muted-foreground">Reset these workspace preferences.</p><Button variant="outline" size="sm" onClick={() => { setRestoreWorkspace(true); setTimestamps(false) }}><RotateCcw data-icon="inline-start" />Restore defaults</Button></div></div>
}

function ThemePreview({ theme }: { theme: Theme }) {
  const canvas = theme === "light" ? "bg-[#f8f3ea]" : theme === "dark" ? "bg-[#3d342e]" : "bg-transparent"
  return <div className={`h-28 overflow-hidden rounded-lg border border-black/10 p-2 ${canvas}`}>{theme === "system" ? <div className="grid h-full grid-cols-2 overflow-hidden rounded-md"><div className="bg-[#f8f3ea]" /><div className="bg-[#3d342e]" /></div> : <div className="grid h-full grid-cols-[1.5rem_1fr] gap-1.5"><div className={theme === "light" ? "rounded-sm bg-[#e9dcc8]" : "rounded-sm bg-[#58493f]"} /><div className="grid gap-1.5"><div className={theme === "light" ? "h-3 rounded-sm bg-[#e0c9aa]" : "h-3 rounded-sm bg-[#5a4a40]"} /><div className={theme === "light" ? "rounded-sm bg-white/90" : "rounded-sm bg-[#493d35]"} /></div></div>}</div>
}

function PreferenceRow({ title, description, checked, onCheckedChange }: { title: string; description: string; checked: boolean; onCheckedChange: (checked: boolean) => void }) { return <div className="flex items-center gap-4 px-4 py-4"><div className="min-w-0 flex-1"><p className="text-xs font-medium">{title}</p><p className="mt-1 text-[11px] leading-4 text-muted-foreground">{description}</p></div><Switch checked={checked} onCheckedChange={onCheckedChange} /></div> }
