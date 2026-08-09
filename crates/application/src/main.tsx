import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Provider as JotaiProvider } from "jotai";
import App from "./App";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./index.css";
import { SidebarProvider } from "./components/ui/sidebar";
import { readPalette } from "@/state";

document.documentElement.dataset.style = readPalette();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <JotaiProvider>
      <TooltipProvider>
        <SidebarProvider className="h-full min-h-0">
          <App />
        </SidebarProvider>
      </TooltipProvider>
    </JotaiProvider>
  </StrictMode>,
);
