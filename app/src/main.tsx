import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/globals.css";
import "dockview-react/dist/styles/dockview.css";
import "./styles/app.css";
import { App } from "./App";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { loadInitialState } from "./core/actions";
import { useEditor } from "./core/store";
import { initTheme } from "./core/theme";

initTheme();
void loadInitialState();
useEditor.getState().log("info", "PCBPlanner Studio started. Create or open a project from the Home tab; the built-in “PCBPlanner Basic” library is ready in the Components panel.");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider delayDuration={400}>
      <App />
      <Toaster position="bottom-right" />
    </TooltipProvider>
  </StrictMode>,
);
