import { Provider } from "jotai";
import App from "./App";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SidebarProvider } from "@/components/ui/sidebar";

export default function WebApp() {
  return (
    <Provider>
      <TooltipProvider>
        <SidebarProvider className="h-full min-h-0">
          <App />
        </SidebarProvider>
      </TooltipProvider>
    </Provider>
  );
}
