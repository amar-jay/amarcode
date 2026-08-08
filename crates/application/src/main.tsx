import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./index.css";
import { SidebarProvider } from "./components/ui/sidebar";

document.documentElement.dataset.style = "ember";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider>
    <SidebarProvider>
      <App />
    </SidebarProvider>
    </TooltipProvider>
  </StrictMode>,
);
