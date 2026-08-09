import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Provider as JotaiProvider } from "jotai";
import App from "./App";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./index.css";
import { SidebarProvider } from "./components/ui/sidebar";
import { readPalette } from "@/hooks/use-theme";

document.documentElement.dataset.style = readPalette();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <JotaiProvider>
      <TooltipProvider>
        <SidebarProvider>
          <App />
        </SidebarProvider>
      </TooltipProvider>
    </JotaiProvider>
  </StrictMode>,
);
