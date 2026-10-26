import { Component, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { useEditor } from "../core/store";

interface Props {
  name: string;
  children: ReactNode;
}

/** Keeps a crash in one panel from taking down the whole window. */
export class PanelErrorBoundary extends Component<Props, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error) {
    useEditor.getState().log("error", `${this.props.name} panel crashed: ${error.message}`);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-panel p-6 text-center text-xs text-muted-foreground">
        <p>
          The {this.props.name} panel hit an error: <span className="font-mono text-err-line">{this.state.error.message}</span>
        </p>
        <Button variant="outline" onClick={() => this.setState({ error: null })}>
          Reload panel
        </Button>
      </div>
    );
  }
}
